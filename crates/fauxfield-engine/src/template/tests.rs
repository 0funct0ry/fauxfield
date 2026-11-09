use super::*;
use serde_json::json;

fn input(seed: Option<u64>, counter: u64) -> Input<'static> {
    Input {
        context: json!({"request": {"method": "GET", "params": {"id": "7"}}, "vars": {"n": 3}}),
        root: Path::new("."),
        seed,
        route_id: "users",
        counter,
        seq: None,
    }
}
fn r(src: &str, seed: Option<u64>) -> String {
    render(src, &input(seed, 1)).unwrap()
}

#[test]
fn plain_text_untouched() {
    assert_eq!(r("hello", None), "hello");
}

#[test]
fn context_and_filters() {
    assert_eq!(r("{{ request.params.id }}-{{ vars.n }}", None), "7-3");
    assert_eq!(r("{{ 'Hello World!' | slugify }}", None), "hello-world");
    assert_eq!(
        r("{{ 'abc' | base64_encode | base64_decode }}", None),
        "abc"
    );
    assert_eq!(r("{{ 5 | pad(width=3, fill='0') }}", None), "005");
    assert_eq!(r("{{ 12.5 | money(currency='USD') }}", None), "$12.50");
    assert_eq!(r("{{ 'abc' | sha256 }}", None).len(), 64);
    assert_eq!(r("{{ [1,2] | json }}", None), "[1,2]");
}

#[test]
fn seeded_is_deterministic_and_unseeded_differs() {
    let t = "{% for _ in repeat(n=20) %}{{ fake(kind='name') }}|{{ uuid() }}|{{ random_int(min=1,max=1000) }};{% endfor %}";
    let a = r(t, Some(42));
    assert_eq!(a, r(t, Some(42)));
    assert_ne!(a, render(t, &input(Some(42), 2)).unwrap());
    assert_ne!(r(t, None), r(t, None));
}

#[test]
fn every_fake_kind_works() {
    for (k, _) in FAKE_KINDS {
        let v = r(&format!("{{{{ fake(kind='{k}') }}}}"), Some(1));
        assert!(!v.is_empty(), "{k}");
    }
    assert!(render("{{ fake(kind='nope') }}", &input(None, 1)).is_err());
}

#[test]
fn functions() {
    assert_eq!(
        r("{{ jsonpath(value=request, path='$.params.id') }}", None),
        "7"
    );
    assert_eq!(r("{{ hmac_sha256(key='k', data='d') }}", None).len(), 64);
    assert_eq!(
        r("{{ jwt(claims=vars, secret='s') }}", None)
            .matches('.')
            .count(),
        2
    );
    assert_eq!(r("{{ pick(from=['x']) }}", None), "x");
    assert_eq!(
        r("{{ weighted(items=['a','b'], weights=[0,1]) }}", None),
        "b"
    );
    assert_eq!(r("{{ lorem(words=3) }}", None).split(' ').count(), 3);
    assert!(r("{{ now(offset='-1d') }}", None).contains('T'));
    assert_eq!(
        r("{{ random_float(min=1.0,max=1.0,precision=1) }}", None),
        "1"
    );
    assert!(render("{{ repeat(n=999999999) }}", &input(None, 1)).is_err());
}

#[test]
fn seq_counts() {
    let seq: Sequences = Default::default();
    let mut i = input(None, 1);
    i.seq = Some(seq);
    assert_eq!(render("{{ seq(name='a') }}", &i).unwrap(), "1");
    assert_eq!(render("{{ seq(name='a') }}", &i).unwrap(), "2");
}

#[test]
fn file_is_confined() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.txt"), "hi").unwrap();
    let mut i = input(None, 1);
    i.root = d.path();
    assert_eq!(render("{{ file(path='a.txt') }}", &i).unwrap(), "hi");
    assert!(render("{{ file(path='../x') }}", &i).is_err());
}

#[test]
fn syntax_errors_have_positions() {
    let e = check_syntax("a\n{{ foo( }}").unwrap_err();
    assert_eq!(e.line, Some(2), "{e:?}");
    assert!(check_syntax("{{ x }}").is_ok());
    assert!(render("{{ x", &input(None, 1)).is_err());
}

#[test]
fn nondeterminism_scan() {
    assert!(is_nondeterministic("{{ uuid() }}"));
    assert!(!is_nondeterministic("{{ request.path }}"));
}
