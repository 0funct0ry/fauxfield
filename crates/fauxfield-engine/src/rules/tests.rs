use std::path::Path;

use fauxfield_model::{Route, parse_server};

use super::*;

fn route(rules_toml: &str, mode: &str) -> Route {
    let t = format!(
        "id=\"srv_t\"\nname=\"t\"\nport=1\n[[routes]]\nid=\"rte_t\"\npath=\"/x\"\nresponse_mode=\"{mode}\"\n\
         [[routes.responses]]\nid=\"hit\"\nname=\"Hit\"\n{rules_toml}\n\
         [[routes.responses]]\nid=\"dflt\"\nname=\"Default\"\ndefault=true\n"
    );
    parse_server(&t).unwrap().routes.remove(0)
}

fn rule(target: &str, key: &str, op: &str, value: &str, extra: &str) -> String {
    let key = if key.is_empty() {
        String::new()
    } else {
        format!("key=\"{key}\"\n")
    };
    let value = if value.is_empty() {
        String::new()
    } else {
        format!("value={value}\n")
    };
    format!("[[routes.responses.rules]]\ntarget=\"{target}\"\n{key}op=\"{op}\"\n{value}{extra}\n")
}

struct Req {
    method: &'static str,
    url: &'static str,
    headers: Vec<(String, String)>,
    params: Vec<(String, String)>,
    body: &'static str,
    call: u64,
}

impl Default for Req {
    fn default() -> Self {
        Req {
            method: "GET",
            url: "/x",
            headers: vec![],
            params: vec![],
            body: "",
            call: 1,
        }
    }
}

fn pick(r: &Route, q: &Req) -> (String, Trace) {
    let ctx = Ctx::new(
        q.method,
        q.url,
        &q.headers,
        &q.params,
        Some("10.0.0.7".into()),
        q.body.as_bytes(),
        q.call,
        Path::new("."),
    );
    let (resp, t) = select(r, &ctx, &Compiled::default());
    (resp.map(|r| r.id.clone()).unwrap_or_default(), t)
}

fn h(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

#[test]
fn every_op_table() {
    // (rule toml, request, should the "hit" response be chosen)
    let json = Req {
        method: "POST",
        headers: vec![h("content-type", "application/json")],
        body: r#"{"user":{"role":"admin","age":30,"tags":["a"]}}"#,
        ..Req::default()
    };
    let q = |url| Req {
        url,
        ..Req::default()
    };
    type Case = (String, Req, bool);
    let cases: Vec<Case> = vec![
        (
            rule("query", "role", "eq", "\"admin\"", ""),
            q("/x?role=admin"),
            true,
        ),
        (
            rule("query", "role", "eq", "\"admin\"", ""),
            q("/x?role=bob"),
            false,
        ),
        (rule("query", "role", "eq", "\"admin\"", ""), q("/x"), false),
        (rule("query", "n", "eq", "5", ""), q("/x?n=5.0"), true),
        (
            rule("query", "role", "ne", "\"admin\"", ""),
            q("/x?role=bob"),
            true,
        ),
        (
            rule("query", "role", "ne", "\"admin\"", ""),
            q("/x?role=admin"),
            false,
        ),
        (rule("query", "role", "ne", "\"admin\"", ""), q("/x"), true),
        (
            rule("query", "q", "contains", "\"ell\"", ""),
            q("/x?q=hello"),
            true,
        ),
        (
            rule("query", "q", "contains", "\"zz\"", ""),
            q("/x?q=hello"),
            false,
        ),
        (
            rule("query", "q", "starts_with", "\"he\"", ""),
            q("/x?q=hello"),
            true,
        ),
        (
            rule("query", "q", "ends_with", "\"lo\"", ""),
            q("/x?q=hello"),
            true,
        ),
        (
            rule("query", "q", "ends_with", "\"he\"", ""),
            q("/x?q=hello"),
            false,
        ),
        (
            rule("query", "q", "regex", "\"^h.l+o$\"", ""),
            q("/x?q=hello"),
            true,
        ),
        (
            rule("query", "q", "regex", "\"^x\"", ""),
            q("/x?q=hello"),
            false,
        ),
        (rule("query", "q", "exists", "", ""), q("/x?q="), true),
        (rule("query", "q", "exists", "", ""), q("/x"), false),
        (rule("query", "q", "absent", "", ""), q("/x"), true),
        (rule("query", "q", "absent", "", ""), q("/x?q=1"), false),
        (rule("query", "n", "gt", "5", ""), q("/x?n=6"), true),
        (rule("query", "n", "gt", "5", ""), q("/x?n=5"), false),
        (rule("query", "n", "gte", "5", ""), q("/x?n=5"), true),
        (rule("query", "n", "lt", "5", ""), q("/x?n=4"), true),
        (rule("query", "n", "lt", "5", ""), q("/x?n=abc"), false),
        (rule("query", "n", "lte", "5", ""), q("/x?n=5"), true),
        (rule("query", "n", "lte", "5", ""), q("/x?n=6"), false),
        (
            rule("query", "c", "in", "[\"a\", \"b\"]", ""),
            q("/x?c=b"),
            true,
        ),
        (
            rule("query", "c", "in", "[\"a\", \"b\"]", ""),
            q("/x?c=z"),
            false,
        ),
        (rule("query", "c", "in", "[1, 2]", ""), q("/x?c=2"), true),
        (
            rule("query", "role", "eq", "\"admin\"", "negate=true"),
            q("/x?role=bob"),
            true,
        ),
        (
            rule("query", "role", "eq", "\"admin\"", "negate=true"),
            q("/x?role=admin"),
            false,
        ),
        (
            rule("method", "", "eq", "\"POST\"", ""),
            Req {
                method: "POST",
                ..Req::default()
            },
            true,
        ),
        (
            rule("path", "", "starts_with", "\"/x\"", ""),
            q("/x?a=1"),
            true,
        ),
        (
            rule("ip", "", "eq", "\"10.0.0.7\"", ""),
            Req::default(),
            true,
        ),
        (
            rule("ip", "", "eq", "\"10.0.0.8\"", ""),
            Req::default(),
            false,
        ),
        (
            rule("count", "", "eq", "3", ""),
            Req {
                call: 3,
                ..Req::default()
            },
            true,
        ),
        (
            rule("count", "", "eq", "3", ""),
            Req {
                call: 2,
                ..Req::default()
            },
            false,
        ),
        (
            rule("header", "Authorization", "absent", "", ""),
            q("/x"),
            true,
        ),
        (
            rule("header", "Authorization", "exists", "", ""),
            Req {
                headers: vec![h("authorization", "Bearer t")],
                ..Req::default()
            },
            true,
        ),
        (
            rule("cookie", "sid", "eq", "\"abc\"", ""),
            Req {
                headers: vec![h("cookie", "a=1; sid=abc")],
                ..Req::default()
            },
            true,
        ),
        (
            rule("param", "id", "eq", "\"42\"", ""),
            Req {
                params: vec![h("id", "42")],
                ..Req::default()
            },
            true,
        ),
        (
            rule("body", "$.user.role", "eq", "\"admin\"", ""),
            Req {
                method: "POST",
                headers: json.headers.clone(),
                body: json.body,
                ..Req::default()
            },
            true,
        ),
        (
            rule("body", "$.user.age", "gte", "18", ""),
            Req {
                method: "POST",
                headers: json.headers.clone(),
                body: json.body,
                ..Req::default()
            },
            true,
        ),
        (
            rule("body", "$.user.missing", "absent", "", ""),
            Req {
                method: "POST",
                headers: json.headers.clone(),
                body: json.body,
                ..Req::default()
            },
            true,
        ),
        (
            rule("body", "name", "eq", "\"ann\"", ""),
            Req {
                method: "POST",
                headers: vec![h("content-type", "application/x-www-form-urlencoded")],
                body: "name=ann&x=1",
                ..Req::default()
            },
            true,
        ),
        (
            rule("body", "name", "eq", "\"ann\"", ""),
            Req {
                method: "POST",
                headers: vec![h("content-type", "multipart/form-data; boundary=BB")],
                body: "--BB\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nann\r\n--BB\r\nContent-Disposition: form-data; name=\"f\"; filename=\"a.txt\"\r\n\r\nZ\r\n--BB--\r\n",
                ..Req::default()
            },
            true,
        ),
        (
            rule("body", "", "contains", "\"ping\"", ""),
            Req {
                method: "POST",
                body: "a ping b",
                ..Req::default()
            },
            true,
        ),
        // Variables and datasets never match until M7/M8.
        (
            rule("variable", "v", "exists", "", ""),
            Req::default(),
            false,
        ),
        (
            rule("dataset", "d.$.x", "exists", "", ""),
            Req::default(),
            false,
        ),
    ];
    for (i, (rules, req, want)) in cases.iter().enumerate() {
        let r = route(rules, "rules");
        let (id, t) = pick(&r, req);
        assert_eq!(id == "hit", *want, "case {i}: {rules}\n{t:#?}");
        if !want {
            assert_eq!(id, "dflt", "case {i} must fall back to default");
        }
    }
}

#[test]
fn and_or_logic_and_trace() {
    let rules = format!(
        "{}{}",
        rule("query", "a", "eq", "\"1\"", ""),
        rule("query", "b", "eq", "\"2\"", "")
    );
    let r = route(&rules, "rules");
    let both = Req {
        url: "/x?a=1&b=2",
        ..Req::default()
    };
    let one = Req {
        url: "/x?a=1",
        ..Req::default()
    };
    assert_eq!(pick(&r, &both).0, "hit");
    let (id, t) = pick(&r, &one);
    assert_eq!(id, "dflt");
    let ev = &t.evaluated[0];
    assert!(ev.rules[0].passed && !ev.rules[1].passed);
    assert_eq!(ev.rules[1].actual, None);
    assert!(t.reason.contains("default"));

    let or_rules = format!("rules_logic=\"or\"\n{rules}");
    let r = route(&or_rules, "rules");
    assert_eq!(pick(&r, &one).0, "hit");
}

#[test]
fn sequential_cycles_and_skips_disabled() {
    let r = route("", "sequential");
    let ids: Vec<String> = (1..=4)
        .map(|c| {
            pick(
                &r,
                &Req {
                    call: c,
                    ..Req::default()
                },
            )
            .0
        })
        .collect();
    assert_eq!(ids, ["hit", "dflt", "hit", "dflt"]);
    let r = route("disabled=true", "sequential");
    assert_eq!(pick(&r, &Req::default()).0, "dflt");
}

#[test]
fn random_stays_in_set_and_disabled_response_is_skipped() {
    let r = route("", "random");
    for _ in 0..20 {
        let id = pick(&r, &Req::default()).0;
        assert!(id == "hit" || id == "dflt");
    }
    let r = route(
        &format!("disabled=true\n{}", rule("query", "a", "exists", "", "")),
        "rules",
    );
    assert_eq!(
        pick(
            &r,
            &Req {
                url: "/x?a=1",
                ..Req::default()
            }
        )
        .0,
        "dflt"
    );
}

#[test]
fn json_schema_op() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("s.json"),
        r#"{"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}"#,
    )
    .unwrap();
    let r = route(&rule("body", "", "json_schema", "\"s.json\"", ""), "rules");
    let pick_with = |body: &'static str| {
        let hs = vec![h("content-type", "application/json")];
        let ctx = Ctx::new("POST", "/x", &hs, &[], None, body.as_bytes(), 1, dir.path());
        select(&r, &ctx, &Compiled::default()).0.unwrap().id.clone()
    };
    assert_eq!(pick_with(r#"{"id":1}"#), "hit");
    assert_eq!(pick_with(r#"{"id":"x"}"#), "dflt");
    assert_eq!(pick_with("nope"), "dflt");
}
