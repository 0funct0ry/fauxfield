//! Generator for `examples/<name>/` (M19.1). Each example is described with the small builder
//! below and written as an ordinary Workspace tree; `cargo xtask examples` rewrites the tree and
//! `cargo xtask check` fails on drift.

use std::fmt::Write as _;
use std::path::Path;

pub type Rule = (&'static str, &'static str, &'static str, &'static str);

pub struct Cb {
    pub name: &'static str,
    pub url: String,
    pub delay_ms: u64,
    pub body: String,
    pub retries: u32,
}

pub struct Rs {
    pub name: String,
    pub status: u16,
    pub body: String,
    pub ctype: &'static str,
    pub delay: Option<&'static str>,
    pub rules: Vec<Rule>,
    pub default: bool,
    pub headers: Vec<(&'static str, String)>,
    pub cbs: Vec<Cb>,
    pub actions: Vec<String>,
}

fn rs(name: &str, status: u16, body: &str) -> Rs {
    Rs {
        name: name.into(),
        status,
        body: body.into(),
        ctype: "application/json",
        delay: None,
        rules: vec![],
        default: false,
        headers: vec![],
        cbs: vec![],
        actions: vec![],
    }
}

pub enum Kind {
    Http,
    Crud(&'static str),
    Ws(
        &'static str,
        Vec<(&'static str, &'static str, &'static str, String)>,
    ),
    Static(&'static str),
}

pub struct Rt {
    pub folder: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    pub resps: Vec<Rs>,
}

/// A default 200 JSON Route.
pub fn route(
    folder: &'static str,
    method: &'static str,
    path: &'static str,
    name: &'static str,
    body: &str,
) -> Rt {
    let mut d = rs("OK", 200, body);
    d.default = true;
    Rt {
        folder,
        method,
        path,
        name,
        kind: Kind::Http,
        resps: vec![d],
    }
}

pub fn get(f: &'static str, p: &'static str, n: &'static str, body: &str) -> Rt {
    route(f, "GET", p, n, body)
}

pub fn post(f: &'static str, p: &'static str, n: &'static str, status: u16, body: &str) -> Rt {
    let mut r = route(f, "POST", p, n, body);
    r.resps[0].status = status;
    r.resps[0].name = "Created".into();
    r
}

pub fn crud(f: &'static str, p: &'static str, n: &'static str, dataset: &'static str) -> Rt {
    Rt {
        folder: f,
        method: "GET",
        path: p,
        name: n,
        kind: Kind::Crud(dataset),
        resps: vec![],
    }
}

/// WebSocket Route; messages are (trigger, every, when-json-path-eq, body).
pub fn ws(
    f: &'static str,
    p: &'static str,
    n: &'static str,
    msgs: Vec<(&'static str, &'static str, &'static str, String)>,
) -> Rt {
    Rt {
        folder: f,
        method: "GET",
        path: p,
        name: n,
        kind: Kind::Ws("script", msgs),
        resps: vec![],
    }
}

pub fn statics(f: &'static str, p: &'static str, n: &'static str, dir: &'static str) -> Rt {
    Rt {
        folder: f,
        method: "GET",
        path: p,
        name: n,
        kind: Kind::Static(dir),
        resps: vec![],
    }
}

impl Rt {
    /// Conditional Response, tried before the default.
    pub fn when(mut self, name: &str, status: u16, rules: &[Rule], body: &str) -> Self {
        let mut r = rs(name, status, body);
        r.rules = rules.to_vec();
        self.resps.insert(self.resps.len() - 1, r);
        self
    }
    pub fn delay(mut self, d: &'static str) -> Self {
        if let Some(r) = self.resps.last_mut() {
            r.delay = Some(d);
        }
        self
    }
    pub fn header(mut self, k: &'static str, v: &str) -> Self {
        if let Some(r) = self.resps.last_mut() {
            r.headers.push((k, v.into()));
        }
        self
    }
    pub fn cb(mut self, cb: Cb) -> Self {
        if let Some(r) = self.resps.last_mut() {
            r.cbs.push(cb);
        }
        self
    }
}

pub struct Ds {
    pub name: &'static str,
    pub rows: u32,
    pub fields: Vec<&'static str>,
}

pub struct Srv {
    pub slug: &'static str,
    pub name: &'static str,
    pub port: u16,
    pub vars: Vec<(&'static str, String)>,
    pub folders: Vec<&'static str>,
    pub datasets: Vec<Ds>,
    pub routes: Vec<Rt>,
}

impl Srv {
    pub fn new(slug: &'static str, name: &'static str, port: u16) -> Self {
        Srv {
            slug,
            name,
            port,
            vars: vec![],
            folders: vec![],
            datasets: vec![],
            routes: vec![],
        }
    }
    pub fn var(mut self, k: &'static str, v: &str) -> Self {
        self.vars.push((k, v.into()));
        self
    }
    pub fn folders(mut self, f: &[&'static str]) -> Self {
        self.folders = f.to_vec();
        self
    }
    /// Field specs: `name:kind[:opts]`, e.g. `id:sequence`, `price:float:5:500`,
    /// `status:one_of:a|b`, `email:fake:internet.email`, `owner:ref:users`.
    pub fn ds(mut self, name: &'static str, rows: u32, fields: &[&'static str]) -> Self {
        self.datasets.push(Ds {
            name,
            rows,
            fields: fields.to_vec(),
        });
        self
    }
    pub fn routes(mut self, rs: Vec<Rt>) -> Self {
        self.routes.extend(rs);
        self
    }
}

pub struct Ex {
    pub name: &'static str,
    pub title: &'static str,
    pub story: &'static str,
    pub servers: Vec<Srv>,
    pub curls: Vec<(&'static str, String)>,
    pub files: Vec<(&'static str, String)>,
}

fn ident(s: &str) -> String {
    s.replace('-', "_")
}

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn lit(s: &str) -> String {
    assert!(!s.contains("'''"), "body contains '''");
    format!("'''\n{s}\n'''")
}

fn field_toml(spec: &str, out: &mut String) {
    let mut p = spec.splitn(3, ':');
    let name = p.next().unwrap();
    let kind = p.next().unwrap_or("string");
    let opt = p.next().unwrap_or("");
    let _ = write!(
        out,
        "\n[[datasets.generator.fields]]\nname = {}\nkind = {}\n",
        q(name),
        q(kind)
    );
    match kind {
        "fake" => {
            let _ = writeln!(out, "fake = {}", q(opt));
        }
        "int" | "float" | "date" | "datetime" if !opt.is_empty() => {
            let (a, b) = opt.split_once(':').unwrap_or((opt, ""));
            let wrap = |v: &str| {
                if kind.starts_with("date") {
                    q(v)
                } else {
                    v.to_string()
                }
            };
            let _ = writeln!(out, "min = {}", wrap(a));
            if !b.is_empty() {
                let _ = writeln!(out, "max = {}", wrap(b));
            }
        }
        "one_of" => {
            let v: Vec<String> = opt.split('|').map(q).collect();
            let _ = writeln!(out, "values = [{}]", v.join(", "));
        }
        "ref" => {
            let _ = writeln!(out, "dataset = {}", q(opt));
        }
        "const" => {
            let _ = writeln!(out, "value = {}", q(opt));
        }
        "template" => {
            let _ = writeln!(out, "expr = {}", q(opt));
        }
        "regex" => {
            let _ = writeln!(out, "pattern = {}", q(opt));
        }
        "sequence" if !opt.is_empty() => {
            let _ = writeln!(out, "start = {opt}");
        }
        "lorem" if !opt.is_empty() => {
            let _ = writeln!(out, "words = {opt}");
        }
        _ => {}
    }
}

fn server_toml(ex: &Ex, s: &Srv) -> String {
    let mut o = String::new();
    let sid = ident(s.slug);
    let _ = write!(
        o,
        "schema = 1\nid = {}\nname = {}\nenabled = true\nhost = \"127.0.0.1\"\nport = {}\nfake_seed = 42\n\n[cors]\nenabled = true\norigins = [\"*\"]\n",
        q(&format!("srv_{}_{}", ident(ex.name), sid)),
        q(s.name),
        s.port
    );
    if !s.vars.is_empty() {
        o.push_str("\n[variables]\n");
        for (k, v) in &s.vars {
            let _ = writeln!(o, "{k} = {}", q(v));
        }
    }
    for d in &s.datasets {
        let inc = d.fields.first().is_some_and(|f| f.contains(":sequence"));
        let _ = write!(
            o,
            "\n[[datasets]]\nname = {}\nkey = \"id\"\nid_strategy = {}\n\n[datasets.generator]\nrows = {}\n",
            q(d.name),
            q(if inc { "increment" } else { "uuid" }),
            d.rows
        );
        for f in &d.fields {
            field_toml(f, &mut o);
        }
    }
    for (i, f) in s.folders.iter().enumerate() {
        let _ = write!(
            o,
            "\n[[folders]]\nid = \"fld_{sid}_{}\"\nname = {}\n",
            i + 1,
            q(f)
        );
    }
    for (i, r) in s.routes.iter().enumerate() {
        let rid = format!("rte_{sid}_{}", i + 1);
        let _ = write!(o, "\n[[routes]]\nid = {}\nname = {}\n", q(&rid), q(r.name));
        if let Some(fi) = s.folders.iter().position(|f| *f == r.folder) {
            let _ = writeln!(o, "folder = \"fld_{sid}_{}\"", fi + 1);
        } else {
            assert!(
                r.folder.is_empty(),
                "unknown folder {} in {}",
                r.folder,
                s.slug
            );
        }
        match &r.kind {
            Kind::Crud(d) => {
                let _ = writeln!(
                    o,
                    "kind = \"crud\"\npath = {}\ndataset = {}",
                    q(r.path),
                    q(d)
                );
            }
            Kind::Static(dir) => {
                let _ = writeln!(
                    o,
                    "kind = \"static\"\npath = {}\ndir = {}\nindex = \"index.html\"",
                    q(r.path),
                    q(dir)
                );
            }
            Kind::Ws(mode, msgs) => {
                let _ = writeln!(
                    o,
                    "kind = \"websocket\"\npath = {}\nmode = {}",
                    q(r.path),
                    q(mode)
                );
                for (trig, every, when, body) in msgs {
                    let _ = write!(o, "\n  [[routes.messages]]\n  trigger = {}\n", q(trig));
                    if !every.is_empty() {
                        let _ = writeln!(o, "  every = {}", q(every));
                    }
                    if !when.is_empty() {
                        let _ = writeln!(
                            o,
                            "  when = {{ target = \"body\", key = \"$.type\", op = \"eq\", value = {} }}",
                            q(when)
                        );
                    }
                    let _ = writeln!(o, "  body = {}", lit(body));
                }
            }
            Kind::Http => {
                let _ = writeln!(o, "method = {}\npath = {}", q(r.method), q(r.path));
            }
        }
        for (j, resp) in r.resps.iter().enumerate() {
            let rsid = format!("rsp_{sid}_{}_{}", i + 1, j + 1);
            let _ = write!(
                o,
                "\n  [[routes.responses]]\n  id = {}\n  name = {}\n  status = {}\n  content_type = {}\n",
                q(&rsid),
                q(&resp.name),
                resp.status,
                q(resp.ctype)
            );
            if resp.default {
                o.push_str("  default = true\n");
            }
            if let Some(d) = resp.delay {
                if d.contains("..") {
                    let _ = writeln!(o, "  delay_ms = {}", q(d));
                } else {
                    let _ = writeln!(o, "  delay_ms = {d}");
                }
            }
            let _ = writeln!(o, "  body = {}", lit(&resp.body));
            for (k, v) in &resp.headers {
                if resp.headers[0].0 == *k {
                    o.push_str("  [routes.responses.headers]\n");
                }
                let _ = writeln!(o, "  {} = {}", q(k), q(v));
            }
            for a in &resp.actions {
                let _ = writeln!(o, "\n    [[routes.responses.actions]]\n    {a}");
            }
            for (ri, ru) in resp.rules.iter().enumerate() {
                let _ = write!(
                    o,
                    "\n    [[routes.responses.rules]]\n    target = {}\n",
                    q(ru.0)
                );
                if !ru.1.is_empty() {
                    let _ = writeln!(o, "    key = {}", q(ru.1));
                }
                let _ = writeln!(o, "    op = {}", q(ru.2));
                if !ru.3.is_empty() {
                    let _ = writeln!(o, "    value = {}", q(ru.3));
                }
                let _ = ri;
            }
            for (ci, cb) in resp.cbs.iter().enumerate() {
                let _ = write!(
                    o,
                    "\n    [[routes.responses.callbacks]]\n    id = \"cb_{sid}_{}_{}\"\n    name = {}\n    method = \"POST\"\n    url = {}\n    delay_ms = {}\n    retries = {}\n    body = {}\n\n      [routes.responses.callbacks.headers]\n      content-type = \"application/json\"\n",
                    i + 1,
                    ci + 1,
                    q(cb.name),
                    q(&cb.url),
                    cb.delay_ms,
                    cb.retries,
                    lit(&cb.body)
                );
            }
        }
    }
    o
}

fn readme(ex: &Ex) -> String {
    let mut o = format!(
        "# {}\n\n{}\n\n## Servers\n\n| Server | Port | Routes |\n| --- | --- | --- |\n",
        ex.title, ex.story
    );
    for s in &ex.servers {
        let _ = writeln!(
            o,
            "| {} (`servers/{}.toml`) | {} | {} |",
            s.name,
            s.slug,
            s.port,
            s.routes.len()
        );
    }
    o.push_str("\nTwo examples are not meant to run together: they reuse the 4001+ port block.\n\n## Run it\n\n```bash\nfauxfield validate\nfauxfield run\n```\n\nOutput is deterministic: every Server sets `fake_seed`.\n\n## Try it\n");
    for (t, c) in &ex.curls {
        let _ = write!(o, "\n{t}:\n\n```bash\n{c}\n```\n");
    }
    o.push_str("\nEnvironments `local` and `staging` (in `environments/`) define `base_url_<server>` Variables.\nAll data is synthetic.\n");
    o
}

fn write_file(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// Render the whole tree as `(relative path, text)`.
pub fn render(ex: &Ex) -> Vec<(String, String)> {
    let mut out = vec![(
        "fauxfield.toml".to_string(),
        format!(
            "schema = 1\nname = \"__WORKSPACE_NAME__\"\ndefault_environment = \"local\"\n\n[variables]\nexample = {}\n\n[defaults]\nhost = \"127.0.0.1\"\nfake_seed = 42\n",
            q(ex.name)
        ),
    )];
    let mut local = String::from("name = \"local\"\n\n[variables]\n");
    let mut stg = String::from("name = \"staging\"\n\n[variables]\n");
    for s in &ex.servers {
        let _ = writeln!(
            local,
            "base_url_{} = {}",
            ident(s.slug),
            q(&format!("http://127.0.0.1:{}", s.port))
        );
        let _ = writeln!(
            stg,
            "base_url_{} = {}",
            ident(s.slug),
            q(&format!("https://{}.staging.example.test", s.slug))
        );
    }
    local.push_str("api_key = \"local-demo-key\"\n");
    stg.push_str("api_key = \"staging-demo-key\"\n");
    out.push(("environments/local.toml".into(), local));
    out.push(("environments/staging.toml".into(), stg));
    for s in &ex.servers {
        out.push((format!("servers/{}.toml", s.slug), server_toml(ex, s)));
    }
    out.push(("docs/README.md".into(), readme(ex)));
    for (p, c) in &ex.files {
        out.push((p.to_string(), c.clone()));
    }
    out
}

pub fn write_all(dir: &Path, exs: &[Ex]) {
    for ex in exs {
        let root = dir.join(ex.name);
        let _ = std::fs::remove_dir_all(&root);
        for (p, t) in render(ex) {
            write_file(&root, &p, &t);
        }
    }
}

/// True when the tree on disk equals the generator output.
pub fn fresh(dir: &Path, exs: &[Ex]) -> bool {
    exs.iter().all(|ex| {
        let root = dir.join(ex.name);
        render(ex)
            .iter()
            .all(|(p, t)| std::fs::read_to_string(root.join(p)).ok().as_deref() == Some(t.as_str()))
    })
}

/// Tera list body of `n` items.
pub fn list(n: u32, item: &str) -> String {
    format!(
        "[{{% for i in range(end={n}) %}}{item}{{% if not loop.last %}},{{% endif %}}{{% endfor %}}]"
    )
}
