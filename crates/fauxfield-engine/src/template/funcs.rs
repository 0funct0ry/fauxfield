//! Template functions and filters (SPEC §8.2).

use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use rand::RngExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tera::{Error, Result, Tera};

use super::{fake, with_state};

type Args = HashMap<String, Value>;

pub const FUNCTIONS: &[(&str, &str)] = &[
    (
        "fake",
        "fake(kind=, locale=) random realistic value; see fake kinds",
    ),
    ("uuid", "uuid() random UUID v4"),
    ("ulid", "ulid() ULID"),
    (
        "now",
        "now(format=, offset=) current time; offset like -1d, +2h, 30m",
    ),
    ("random_int", "random_int(min=, max=) inclusive"),
    ("random_float", "random_float(min=, max=, precision=)"),
    ("pick", "pick(from=[...]) one random item"),
    ("weighted", "weighted(items=[...], weights=[...])"),
    ("seq", "seq(name=) 1, 2, 3... per name until state reset"),
    ("repeat", "repeat(n=) array of n items for loops"),
    ("jsonpath", "jsonpath(value=, path=) first match or null"),
    ("hmac_sha256", "hmac_sha256(key=, data=) hex digest"),
    ("jwt", "jwt(claims=, secret=) HS256 token"),
    ("lorem", "lorem(words=) filler text"),
    ("file", "file(path=) workspace-relative file contents"),
    (
        "dataset_rows",
        "dataset_rows(data=data.orders, filter=\"status=open&total_gte=5\", sort=\"total\", order=\"desc\", page=1, limit=10) filtered, sorted, paged rows of a Dataset",
    ),
    (
        "dataset_count",
        "dataset_count(data=data.orders, filter=\"status=open\") number of rows matching a filter",
    ),
];

pub const FILTERS: &[(&str, &str)] = &[
    ("json", "compact JSON"),
    ("pretty_json", "indented JSON"),
    ("base64_encode", "standard base64"),
    ("base64_decode", "decode standard base64"),
    ("url_encode", "percent-encode"),
    ("slugify", "lower-case, dash separated"),
    ("sha256", "hex SHA-256"),
    ("pad", "pad(width=, fill=\" \") left pad"),
    ("money", "money(currency=\"USD\") two decimals with symbol"),
];

fn arg<'a>(a: &'a Args, k: &str) -> Result<&'a Value> {
    a.get(k)
        .ok_or_else(|| Error::msg(format!("missing argument `{k}`")))
}
fn str_arg<'a>(a: &'a Args, k: &str) -> Result<&'a str> {
    arg(a, k)?
        .as_str()
        .ok_or_else(|| Error::msg(format!("`{k}` must be a string")))
}
fn i64_arg(a: &Args, k: &str) -> Result<i64> {
    arg(a, k)?
        .as_i64()
        .ok_or_else(|| Error::msg(format!("`{k}` must be an integer")))
}
fn f64_arg(a: &Args, k: &str) -> Result<f64> {
    arg(a, k)?
        .as_f64()
        .ok_or_else(|| Error::msg(format!("`{k}` must be a number")))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn parse_offset(s: &str) -> Result<chrono::Duration> {
    let s = s.trim();
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let unit = rest
        .chars()
        .last()
        .ok_or_else(|| Error::msg("empty offset"))?;
    let n: i64 = rest[..rest.len() - 1]
        .parse()
        .map_err(|_| Error::msg(format!("bad offset `{s}`")))?;
    let n = n * sign;
    Ok(match unit {
        's' => chrono::Duration::seconds(n),
        'm' => chrono::Duration::minutes(n),
        'h' => chrono::Duration::hours(n),
        'd' => chrono::Duration::days(n),
        'w' => chrono::Duration::weeks(n),
        _ => {
            return Err(Error::msg(format!(
                "bad offset unit in `{s}` (use s, m, h, d, w)"
            )));
        }
    })
}

fn b64url(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}

pub fn register(t: &mut Tera) {
    t.register_function("dataset_rows", |a: &Args| {
        let get = |k: &str| a.get(k).and_then(Value::as_u64).unwrap_or(0) as usize;
        let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("");
        Ok(Value::Array(crate::data::query(
            arg(a, "data")?,
            s("filter"),
            s("sort"),
            s("order"),
            get("page").max(1),
            get("limit"),
        )))
    });
    t.register_function("dataset_count", |a: &Args| {
        let f = a.get("filter").and_then(Value::as_str).unwrap_or("");
        let rows = arg(a, "data")?;
        Ok(json!(
            crate::data::matching(rows, &crate::data::parse_filters(f)).len()
        ))
    });
    t.register_function("fake", |a: &Args| {
        let kind = str_arg(a, "kind")?;
        if let Some(l) = a.get("locale").and_then(Value::as_str)
            && !l.eq_ignore_ascii_case("en")
            && !l.eq_ignore_ascii_case("en_US")
        {
            return Err(Error::msg(format!(
                "locale `{l}` is not supported (only `en`)"
            )));
        }
        with_state(|s| fake::generate(kind, &mut s.rng))?
            .map(Value::String)
            .ok_or_else(|| Error::msg(format!("unknown fake kind `{kind}`")))
    });
    t.register_function("uuid", |_: &Args| {
        with_state(|s| {
            let b: [u8; 16] = s.rng.random();
            Value::String(uuid::Builder::from_random_bytes(b).into_uuid().to_string())
        })
    });
    t.register_function("ulid", |_: &Args| {
        with_state(|s| {
            let ts = if s.seeded {
                0
            } else {
                chrono::Utc::now().timestamp_millis() as u64
            };
            let r: u128 = s.rng.random::<u128>() & ((1u128 << 80) - 1);
            Value::String(ulid::Ulid::from_parts(ts, r).to_string())
        })
    });
    t.register_function("now", |a: &Args| {
        let mut n = chrono::Utc::now();
        if let Some(o) = a.get("offset").and_then(Value::as_str) {
            n += parse_offset(o)?;
        }
        Ok(Value::String(
            match a.get("format").and_then(Value::as_str) {
                Some("unix") => n.timestamp().to_string(),
                Some(f) => {
                    use std::fmt::Write;
                    let mut s = String::new();
                    write!(s, "{}", n.format(f)).map_err(|_| Error::msg("bad time format"))?;
                    s
                }
                None => n.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            },
        ))
    });
    t.register_function("random_int", |a: &Args| {
        let (lo, hi) = (i64_arg(a, "min")?, i64_arg(a, "max")?);
        if lo > hi {
            return Err(Error::msg("random_int: min > max"));
        }
        with_state(|s| json!(s.rng.random_range(lo..=hi)))
    });
    t.register_function("random_float", |a: &Args| {
        let (lo, hi) = (f64_arg(a, "min")?, f64_arg(a, "max")?);
        let p = a
            .get("precision")
            .and_then(Value::as_i64)
            .unwrap_or(2)
            .clamp(0, 12) as i32;
        if lo > hi {
            return Err(Error::msg("random_float: min > max"));
        }
        with_state(|s| {
            let v = lo + s.rng.random::<f64>() * (hi - lo);
            let m = 10f64.powi(p);
            json!((v * m).round() / m)
        })
    });
    t.register_function("pick", |a: &Args| {
        let items = arg(a, "from")?
            .as_array()
            .ok_or_else(|| Error::msg("`from` must be an array"))?;
        if items.is_empty() {
            return Err(Error::msg("pick: `from` is empty"));
        }
        with_state(|s| items[s.rng.random_range(0..items.len())].clone())
    });
    t.register_function("weighted", |a: &Args| {
        let items = arg(a, "items")?
            .as_array()
            .ok_or_else(|| Error::msg("`items` must be an array"))?;
        let w: Vec<f64> = arg(a, "weights")?
            .as_array()
            .ok_or_else(|| Error::msg("`weights` must be an array"))?
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0).max(0.0))
            .collect();
        let total: f64 = w.iter().sum();
        if items.is_empty() || items.len() != w.len() || total <= 0.0 {
            return Err(Error::msg(
                "weighted: items and weights must match and sum above 0",
            ));
        }
        with_state(|s| {
            let mut x = s.rng.random::<f64>() * total;
            for (i, wi) in w.iter().enumerate() {
                if x < *wi {
                    return items[i].clone();
                }
                x -= wi;
            }
            items[items.len() - 1].clone()
        })
    });
    t.register_function("seq", |a: &Args| {
        let name = str_arg(a, "name")?.to_string();
        with_state(|s| {
            let Some(m) = &s.seq else { return json!(1) };
            let mut m = m.lock().unwrap();
            let e = m.entry(name).or_insert(0);
            *e += 1;
            json!(*e)
        })
    });
    t.register_function("repeat", |a: &Args| {
        let n = i64_arg(a, "n")?;
        if !(0..=100_000).contains(&n) {
            return Err(Error::msg("repeat: n must be between 0 and 100000"));
        }
        Ok(Value::Array(vec![Value::Null; n as usize]))
    });
    t.register_function("jsonpath", |a: &Args| {
        let path = serde_json_path::JsonPath::parse(str_arg(a, "path")?)
            .map_err(|e| Error::msg(format!("jsonpath: {e}")))?;
        let value = arg(a, "value")?;
        Ok(path.query(value).first().cloned().unwrap_or(Value::Null))
    });
    t.register_function("hmac_sha256", |a: &Args| {
        let mut m = Hmac::<Sha256>::new_from_slice(str_arg(a, "key")?.as_bytes())
            .map_err(|e| Error::msg(e.to_string()))?;
        m.update(str_arg(a, "data")?.as_bytes());
        Ok(Value::String(hex(&m.finalize().into_bytes())))
    });
    t.register_function("jwt", |a: &Args| {
        let head = b64url(br#"{"alg":"HS256","typ":"JWT"}"#);
        let claims = b64url(
            serde_json::to_string(arg(a, "claims")?)
                .unwrap_or_default()
                .as_bytes(),
        );
        let signing = format!("{head}.{claims}");
        let mut m = Hmac::<Sha256>::new_from_slice(str_arg(a, "secret")?.as_bytes())
            .map_err(|e| Error::msg(e.to_string()))?;
        m.update(signing.as_bytes());
        Ok(Value::String(format!(
            "{signing}.{}",
            b64url(&m.finalize().into_bytes())
        )))
    });
    t.register_function("lorem", |a: &Args| {
        let n = a
            .get("words")
            .and_then(Value::as_i64)
            .unwrap_or(10)
            .clamp(1, 10_000) as usize;
        with_state(|s| {
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                out.push(fake::generate("lorem.word", &mut s.rng).unwrap_or_default());
            }
            Value::String(out.join(" "))
        })
    });
    t.register_function("file", |a: &Args| {
        let p = str_arg(a, "path")?;
        let root = with_state(|s| s.root.clone())?;
        let b = crate::engine::read_confined(&root, p)
            .map_err(|e| Error::msg(format!("file(`{p}`): {e}")))?;
        Ok(Value::String(String::from_utf8_lossy(&b).into_owned()))
    });

    t.register_filter("json", |v: &Value, _: &Args| {
        Ok(Value::String(
            serde_json::to_string(v).map_err(|e| Error::msg(e.to_string()))?,
        ))
    });
    t.register_filter("pretty_json", |v: &Value, _: &Args| {
        Ok(Value::String(
            serde_json::to_string_pretty(v).map_err(|e| Error::msg(e.to_string()))?,
        ))
    });
    t.register_filter("base64_encode", |v: &Value, _: &Args| {
        Ok(Value::String(STANDARD.encode(text(v))))
    });
    t.register_filter("base64_decode", |v: &Value, _: &Args| {
        let b = STANDARD
            .decode(text(v))
            .map_err(|e| Error::msg(format!("base64_decode: {e}")))?;
        Ok(Value::String(String::from_utf8_lossy(&b).into_owned()))
    });
    t.register_filter("url_encode", |v: &Value, _: &Args| {
        Ok(Value::String(urlencoding::encode(&text(v)).into_owned()))
    });
    t.register_filter("slugify", |v: &Value, _: &Args| {
        let mut out = String::new();
        for c in text(v).chars() {
            if c.is_alphanumeric() {
                out.extend(c.to_lowercase());
            } else if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        }
        Ok(Value::String(out.trim_end_matches('-').to_string()))
    });
    t.register_filter("sha256", |v: &Value, _: &Args| {
        Ok(Value::String(hex(&Sha256::digest(text(v).as_bytes()))))
    });
    t.register_filter("pad", |v: &Value, a: &Args| {
        let w = i64_arg(a, "width")?.clamp(0, 10_000) as usize;
        let fill = a
            .get("fill")
            .and_then(Value::as_str)
            .and_then(|s| s.chars().next())
            .unwrap_or(' ');
        let s = text(v);
        let n = s.chars().count();
        let mut out: String = std::iter::repeat_n(fill, w.saturating_sub(n)).collect();
        out.push_str(&s);
        Ok(Value::String(out))
    });
    t.register_filter("money", |v: &Value, a: &Args| {
        let n = v
            .as_f64()
            .or_else(|| text(v).parse().ok())
            .ok_or_else(|| Error::msg("money: not a number"))?;
        let cur = a.get("currency").and_then(Value::as_str).unwrap_or("USD");
        let sym = match cur {
            "USD" => "$",
            "EUR" => "€",
            "GBP" => "£",
            "JPY" => "¥",
            "INR" => "₹",
            _ => "",
        };
        Ok(Value::String(if sym.is_empty() {
            format!("{cur} {n:.2}")
        } else {
            format!("{sym}{n:.2}")
        }))
    });
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
