//! Seed generators (M10.02).

use std::path::Path;

use fauxfield_engine::generate::{Refs, generate, preview_spec};
use fauxfield_model::{Dataset, GeneratorField, GeneratorSeed, GeneratorSpec, parse_server};
use serde_json::{Value, json};

fn spec(kind: &str) -> GeneratorSpec {
    GeneratorSpec::of(kind)
}

fn field(name: &str, s: GeneratorSpec) -> GeneratorField {
    GeneratorField {
        name: name.into(),
        spec: s,
    }
}

fn one(s: GeneratorSpec, seed: Option<u64>, n: u32) -> Vec<Value> {
    preview_spec(&s, seed, n, &Refs::new(), Path::new(".")).unwrap()
}

#[test]
fn every_generator_produces_its_type() {
    let mut s = spec("int");
    s.min = Some(json!(5));
    s.max = Some(json!(9));
    for v in one(s, Some(1), 50) {
        assert!((5..=9).contains(&v.as_i64().unwrap()));
    }
    let mut s = spec("int");
    s.min = Some(json!(0));
    s.max = Some(json!(100));
    s.step = Some(10.0);
    assert!(
        one(s, Some(1), 30)
            .iter()
            .all(|v| v.as_i64().unwrap() % 10 == 0)
    );
    let mut s = spec("float");
    s.min = Some(json!(1));
    s.max = Some(json!(2));
    s.precision = Some(1);
    assert!(one(s, Some(1), 30).iter().all(|v| {
        let f = v.as_f64().unwrap();
        (1.0..=2.0).contains(&f) && ((f * 10.0).round() - f * 10.0).abs() < 1e-9
    }));
    let mut s = spec("bool");
    s.probability = Some(1.0);
    assert!(one(s, None, 10).iter().all(|v| v == &json!(true)));
    assert_eq!(one(spec("uuid"), None, 1)[0].as_str().unwrap().len(), 36);
    assert_eq!(one(spec("ulid"), None, 1)[0].as_str().unwrap().len(), 26);
    let mut s = spec("sequence");
    s.start = Some(10.0);
    s.step = Some(5.0);
    assert_eq!(one(s, None, 3), vec![json!(10), json!(15), json!(20)]);
    let mut s = spec("regex");
    s.pattern = Some("[A-Z]{3}-\\d{4}".into());
    let re = regex::Regex::new("^[A-Z]{3}-[0-9]{4}$").unwrap();
    assert!(
        one(s, Some(3), 20)
            .iter()
            .all(|v| re.is_match(v.as_str().unwrap()))
    );
    let mut s = spec("one_of");
    s.values = Some(vec![json!("a"), json!("b")]);
    s.weights = Some(vec![0.0, 1.0]);
    assert!(one(s, None, 20).iter().all(|v| v == "b"));
    let mut s = spec("string");
    s.length = Some(6);
    s.charset = Some("xy".into());
    assert!(one(s, None, 5).iter().all(
        |v| v.as_str().unwrap().chars().all(|c| c == 'x' || c == 'y')
            && v.as_str().unwrap().len() == 6
    ));
    let mut s = spec("lorem");
    s.words = Some(4);
    assert_eq!(one(s, None, 1)[0].as_str().unwrap().split(' ').count(), 4);
    let mut s = spec("fake");
    s.fake = Some("internet.email".into());
    assert!(
        one(s, None, 3)
            .iter()
            .all(|v| v.as_str().unwrap().contains('@'))
    );
    let mut s = spec("datetime");
    s.min = Some(json!("2020-01-01T00:00:00Z"));
    s.max = Some(json!("2020-01-02T00:00:00Z"));
    assert!(
        one(s, None, 5)
            .iter()
            .all(|v| v.as_str().unwrap().starts_with("2020-01-0"))
    );
    let mut s = spec("date");
    s.min = Some(json!("2021-05-01"));
    s.max = Some(json!("2021-05-01"));
    assert_eq!(one(s, None, 1)[0], json!("2021-05-01"));
    let mut s = spec("const");
    s.value = Some(json!({"k": 1}));
    assert_eq!(one(s, None, 1)[0], json!({"k": 1}));
    let mut s = spec("null_or");
    s.null_probability = Some(1.0);
    s.inner = Some(Box::new(spec("uuid")));
    assert!(one(s, None, 5).iter().all(Value::is_null));
    let mut s = spec("template");
    s.expr = Some("row.a * 2".into());
    let g = GeneratorSeed {
        rows: 2,
        fields: vec![
            field("a", {
                let mut c = spec("const");
                c.value = Some(json!(21));
                c
            }),
            field("b", s),
        ],
    };
    let v = generate("t", &g, Some(1), None, &Refs::new(), Path::new(".")).unwrap();
    assert_eq!(v[0]["b"], json!(42));
}

#[test]
fn nested_object_and_array() {
    let mut a = spec("array");
    a.min = Some(json!(2));
    a.max = Some(json!(2));
    a.item = Some(Box::new(spec("uuid")));
    let mut o = spec("object");
    o.fields = Some(vec![field("tags", a)]);
    let v = one(o, None, 1);
    assert_eq!(v[0]["tags"].as_array().unwrap().len(), 2);
}

fn users() -> GeneratorSeed {
    let mut e = spec("fake");
    e.fake = Some("internet.email".into());
    let mut r = spec("regex");
    r.pattern = Some("[a-z]{5}".into());
    GeneratorSeed {
        rows: 20,
        fields: vec![
            field("id", spec("uuid")),
            field("ulid", spec("ulid")),
            field("email", e),
            field("code", r),
        ],
    }
}

#[test]
fn same_seed_is_byte_identical() {
    let g = users();
    let a = generate("users", &g, Some(42), None, &Refs::new(), Path::new(".")).unwrap();
    let b = generate("users", &g, Some(42), None, &Refs::new(), Path::new(".")).unwrap();
    let c = generate("users", &g, Some(43), None, &Refs::new(), Path::new(".")).unwrap();
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
    assert_ne!(a, c);
    assert_eq!(a.as_array().unwrap().len(), 20);
}

#[test]
fn bad_regex_names_its_field() {
    let mut r = spec("regex");
    r.pattern = Some("(".into());
    let g = GeneratorSeed {
        rows: 1,
        fields: vec![field("ok", spec("uuid")), field("code", r)],
    };
    let e = generate("d", &g, None, None, &Refs::new(), Path::new(".")).unwrap_err();
    assert_eq!(e.path, "generator.fields[1]");
    assert!(e.message.contains("invalid regex"));
}

fn server(extra: &str) -> fauxfield_model::Server {
    parse_server(&format!(
        "id=\"srv_1\"\nname=\"s\"\nport=1\nfake_seed=7\n{extra}"
    ))
    .unwrap()
}

#[test]
fn toml_roundtrip_and_ref_order() {
    let s = server(
        r#"
[[datasets]]
name = "orders"
[datasets.generator]
rows = 5
[[datasets.generator.fields]]
name = "user"
kind = "ref"
dataset = "users"
[[datasets]]
name = "users"
[datasets.generator]
rows = 3
[[datasets.generator.fields]]
name = "id"
kind = "sequence"
"#,
    );
    let order = fauxfield_model::generator::load_order(&s.datasets).unwrap();
    assert_eq!(order, vec![1, 0]); // users first
    let users = generate(
        "users",
        s.datasets[1].generator.as_ref().unwrap(),
        s.fake_seed,
        None,
        &Refs::new(),
        Path::new("."),
    )
    .unwrap();
    let mut refs = Refs::new();
    refs.insert("users".into(), users.as_array().unwrap().clone());
    let orders = generate(
        "orders",
        s.datasets[0].generator.as_ref().unwrap(),
        s.fake_seed,
        None,
        &refs,
        Path::new("."),
    )
    .unwrap();
    assert!(
        orders
            .as_array()
            .unwrap()
            .iter()
            .all(|o| (1..=3).contains(&o["user"].as_i64().unwrap()))
    );
}

#[test]
fn ref_cycle_is_a_diagnostic() {
    let s = server(
        r#"
[[datasets]]
name = "a"
[datasets.generator]
rows = 1
[[datasets.generator.fields]]
name = "x"
kind = "ref"
dataset = "b"
[[datasets]]
name = "b"
[datasets.generator]
rows = 1
[[datasets.generator.fields]]
name = "x"
kind = "ref"
dataset = "a"
"#,
    );
    let diags = fauxfield_model::validate_servers(&[("s.toml".to_string(), s)]);
    assert!(diags.iter().any(|d| d.code == "E077"));
}

#[test]
fn model_diagnostics_have_field_paths() {
    let s = server(
        r#"
[[datasets]]
name = "a"
seed = "data/a.json"
[datasets.generator]
rows = 1
[[datasets.generator.fields]]
name = "x"
kind = "regex"
pattern = "("
[[datasets.generator.fields]]
name = "y"
kind = "nope"
"#,
    );
    let diags = fauxfield_model::validate_servers(&[("s.toml".to_string(), s)]);
    assert!(diags.iter().any(|d| d.code == "E075"));
    assert!(
        diags
            .iter()
            .any(|d| d.code == "E076" && d.path.contains("fields[0]"))
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == "E076" && d.path.contains("fields[1]"))
    );
    let _ = Dataset::default();
}

#[test]
fn seed_file_edit_reloads_only_that_dataset() {
    use fauxfield_engine::{Engine, EngineConfig};
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::write(dir.path().join("data/a.json"), r#"[{"id":1}]"#).unwrap();
    std::fs::write(dir.path().join("data/b.json"), r#"[{"id":1}]"#).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let e = Engine::new(
        rt.handle().clone(),
        EngineConfig {
            root: dir.path().to_path_buf(),
            ..Default::default()
        },
    );
    let s = server(
        "[[datasets]]\nname=\"a\"\nseed=\"data/a.json\"\n[[datasets]]\nname=\"b\"\nseed=\"data/b.json\"\n[[datasets]]\nname=\"g\"\n[datasets.generator]\nrows=3\n[[datasets.generator.fields]]\nname=\"id\"\nkind=\"uuid\"\n",
    );
    e.apply(vec![s.clone()]);
    let g1 = e.dataset_rows("srv_1", "g").unwrap();
    assert_eq!(g1.as_array().unwrap().len(), 3);
    // live edit of b survives a reload that only touches a's seed file
    e.dataset_set("srv_1", "b", serde_json::json!([{"id": 9}, {"id": 10}]))
        .unwrap();
    std::fs::write(dir.path().join("data/a.json"), r#"[{"id":1},{"id":2}]"#).unwrap();
    e.apply(vec![s]);
    assert_eq!(
        e.dataset_rows("srv_1", "a")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        e.dataset_rows("srv_1", "b")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        e.dataset_rows("srv_1", "g").unwrap(),
        g1,
        "unchanged generator kept its rows"
    );
}
