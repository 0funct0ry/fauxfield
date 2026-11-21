//! M19.1: the embedded example workspaces.

use std::collections::HashSet;

use fauxfield_model::Severity;
use fauxfield_store::{Store, examples};

#[test]
fn registry_has_the_ten_examples_with_the_content_bar() {
    let names: Vec<&str> = examples::EXAMPLES.iter().map(|m| m.name).collect();
    assert_eq!(
        names,
        [
            "ecommerce",
            "banking",
            "social",
            "food-delivery",
            "ride-hailing",
            "saas-billing",
            "healthcare",
            "iot-fleet",
            "streaming-media",
            "travel-booking"
        ]
    );
    for e in examples::list_examples() {
        assert!(
            (3..=6).contains(&e.servers),
            "{}: {} servers",
            e.name,
            e.servers
        );
        assert!(
            !e.description.is_empty() && !e.title.is_empty(),
            "{}",
            e.name
        );
        for s in examples::example_servers(&e.name) {
            assert!(
                (6..=15).contains(&s.routes),
                "{}/{}: {} routes",
                e.name,
                s.slug,
                s.routes
            );
        }
    }
}

#[test]
fn every_example_has_its_layout_and_validates_clean() {
    for m in examples::EXAMPLES {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ws");
        std::fs::create_dir(&root).unwrap();
        let rep = examples::init_example(&root, m.name, false).unwrap();
        assert!(rep.skipped.is_empty());
        for f in [
            "fauxfield.toml",
            "environments/local.toml",
            "environments/staging.toml",
            "docs/README.md",
            ".gitignore",
        ] {
            assert!(root.join(f).is_file(), "{}: missing {f}", m.name);
        }
        let readme = std::fs::read_to_string(root.join("docs/README.md")).unwrap();
        assert!(
            readme.matches("curl ").count() >= 3,
            "{}: README needs 3 curl commands",
            m.name
        );
        let store = Store::open(&root).unwrap();
        let snap = store.snapshot();
        let bad: Vec<String> = snap
            .diagnostics
            .iter()
            .map(|d| format!("{:?} {} {} {}", d.severity, d.code, d.file, d.message))
            .collect();
        assert!(bad.is_empty(), "{}: {bad:?}", m.name);
        assert!(
            snap.diagnostics
                .iter()
                .all(|d| d.severity != Severity::Error)
        );
        assert_eq!(snap.manifest.name, "ws");
        // Every Server has a 4xx/5xx Response, synthetic seed, deterministic generator.
        for (file, s) in &snap.servers {
            assert!(s.fake_seed.is_some(), "{}/{file}: fake_seed", m.name);
            let has_err = s
                .routes
                .iter()
                .flat_map(|r| &r.responses)
                .any(|r| r.status >= 400);
            assert!(has_err, "{}/{file}: no error Response", m.name);
            assert!(
                s.routes.iter().any(|r| r.folder.is_some()),
                "{}/{file}: no Folder",
                m.name
            );
        }
        assert!(snap.environments.len() >= 2, "{}", m.name);
    }
}

#[test]
fn ports_slugs_and_ids_are_unique() {
    let (mut all_ids, mut names) = (HashSet::new(), HashSet::new());
    for m in examples::EXAMPLES {
        assert!(names.insert(m.name));
        let servers = examples::example_servers(m.name);
        let ports: Vec<u16> = servers.iter().map(|s| s.port).collect();
        let slugs: HashSet<&str> = servers.iter().map(|s| s.slug.as_str()).collect();
        assert_eq!(slugs.len(), servers.len(), "{}: slugs", m.name);
        let set: HashSet<&u16> = ports.iter().collect();
        assert_eq!(set.len(), ports.len(), "{}: ports", m.name);
        let lo = *ports.iter().min().unwrap();
        assert_eq!(lo, 4001, "{}: block starts at 4001", m.name);
        assert_eq!(
            *ports.iter().max().unwrap() as usize,
            4000 + ports.len(),
            "{}: contiguous",
            m.name
        );
        // Ids are unique inside an example (and carry the example name, so across all of them).
        for (p, bytes) in examples::example_files(m.name) {
            if !p.starts_with("servers/") {
                continue;
            }
            let v: toml::Value = toml::from_str(std::str::from_utf8(bytes).unwrap()).unwrap();
            assert!(
                all_ids.insert(v["id"].as_str().unwrap().to_string()),
                "{}: server id",
                m.name
            );
        }
    }
}

#[test]
fn existing_files_are_never_changed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ws");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("fauxfield.toml"), "mine = true\n").unwrap();
    std::fs::write(root.join("notes.txt"), "keep\n").unwrap();
    // Conflict without force: refused, nothing written.
    let err = examples::init_example(&root, "banking", false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("fauxfield.toml"), "{err}");
    assert!(!root.join("servers").exists());
    // With force: the conflicting file is skipped and unchanged.
    let rep = examples::init_example(&root, "banking", true).unwrap();
    assert_eq!(rep.skipped, ["fauxfield.toml"]);
    assert_eq!(
        std::fs::read_to_string(root.join("fauxfield.toml")).unwrap(),
        "mine = true\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).unwrap(),
        "keep\n"
    );
    assert!(root.join("servers").is_dir());
    assert!(examples::init_example(&root, "nope", false).is_err());
}

#[test]
fn examples_stay_small() {
    // Raw size is a safe upper bound for the compressed size the budget talks about.
    let total: usize = examples::EXAMPLES
        .iter()
        .flat_map(|m| examples::example_files(m.name))
        .map(|(_, b)| b.len())
        .sum();
    assert!(total < 2 * 1024 * 1024, "examples are {total} bytes");
}
