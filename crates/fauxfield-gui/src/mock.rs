//! Static mock Workspace used until M2 loads real files. Deleted by M2 (standing rule 2: the GUI
//! must not hold state that is not in a file). Everything is deterministic.

#[derive(Clone, Debug, PartialEq)]
pub struct MockResponse {
    pub name: String,
    pub status: u16,
    pub default: bool,
    pub content_type: String,
    pub delay_ms: u32,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub id: String,
    pub method: String,
    pub path: String,
    pub name: String,
    pub responses: Vec<MockResponse>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub routes: Vec<Route>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub prefix: String,
    pub running: bool,
    pub folders: Vec<Folder>,
    pub routes: Vec<Route>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarSource {
    File,
    Dotenv,
    Process,
}

impl VarSource {
    pub fn label(self) -> &'static str {
        match self {
            VarSource::File => "file",
            VarSource::Dotenv => "dotenv",
            VarSource::Process => "process env",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub key: String,
    pub value: String,
    pub source: VarSource,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    pub name: String,
    pub vars: Vec<Variable>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub name: String,
    pub servers: Vec<Server>,
    pub environments: Vec<Environment>,
    pub datasets: Vec<String>,
}

impl Server {
    pub fn all_routes(&self) -> impl Iterator<Item = &Route> {
        self.folders
            .iter()
            .flat_map(|f| f.routes.iter())
            .chain(self.routes.iter())
    }
}

impl Workspace {
    pub fn find_route(&self, id: &str) -> Option<(&Server, &Route)> {
        self.servers
            .iter()
            .find_map(|s| s.all_routes().find(|r| r.id == id).map(|r| (s, r)))
    }

    pub fn find_server(&self, id: &str) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    pub fn find_environment(&self, name: &str) -> Option<&Environment> {
        self.environments.iter().find(|e| e.name == name)
    }

    pub fn running_count(&self) -> usize {
        self.servers.iter().filter(|s| s.running).count()
    }
}

fn resp(name: &str, status: u16, default: bool, body: &str) -> MockResponse {
    MockResponse {
        name: name.into(),
        status,
        default,
        content_type: "application/json".into(),
        delay_ms: 0,
        body: body.into(),
    }
}

fn route(id: &str, method: &str, path: &str, name: &str, responses: Vec<MockResponse>) -> Route {
    Route {
        id: id.into(),
        method: method.into(),
        path: path.into(),
        name: name.into(),
        responses,
    }
}

fn var(key: &str, value: &str, source: VarSource) -> Variable {
    Variable {
        key: key.into(),
        value: value.into(),
        source,
    }
}

pub fn workspace() -> Workspace {
    let user_body = "{\n  \"id\": \"{{ request.params.id }}\",\n  \"name\": \"{{ fake(kind='name.full') }}\",\n  \"email\": \"{{ fake(kind='internet.email') }}\"\n}";
    let users = Server {
        id: "srv_users".into(),
        name: "Users API".into(),
        host: "127.0.0.1".into(),
        port: 4010,
        prefix: "/api".into(),
        running: true,
        folders: vec![Folder {
            id: "fld_users".into(),
            name: "Users".into(),
            routes: vec![
                route(
                    "rt_users_list",
                    "GET",
                    "/users",
                    "List users",
                    vec![resp("200 OK", 200, true, "[]")],
                ),
                route(
                    "rt_users_get",
                    "GET",
                    "/users/:id",
                    "Get user",
                    vec![
                        resp("200 OK", 200, true, user_body),
                        resp(
                            "404 Not found",
                            404,
                            false,
                            "{ \"title\": \"Not found\", \"status\": 404 }",
                        ),
                    ],
                ),
                route(
                    "rt_users_create",
                    "POST",
                    "/users",
                    "Create user",
                    vec![resp("201 Created", 201, true, "{ \"id\": \"usr_1\" }")],
                ),
                route(
                    "rt_users_update",
                    "PATCH",
                    "/users/:id",
                    "Update user",
                    vec![resp("200 OK", 200, true, "{}")],
                ),
                route(
                    "rt_users_delete",
                    "DELETE",
                    "/users/:id",
                    "Delete user",
                    vec![resp("204 No content", 204, true, "")],
                ),
            ],
        }],
        routes: vec![route(
            "rt_health",
            "GET",
            "/health",
            "Health",
            vec![resp("200 OK", 200, true, "{ \"ok\": true }")],
        )],
    };
    let payments = Server {
        id: "srv_payments".into(),
        name: "Payments".into(),
        host: "127.0.0.1".into(),
        port: 4020,
        prefix: String::new(),
        running: false,
        folders: vec![Folder {
            id: "fld_charges".into(),
            name: "Charges".into(),
            routes: vec![
                route(
                    "rt_charges_create",
                    "POST",
                    "/charges",
                    "Create charge",
                    vec![resp(
                        "202 Accepted",
                        202,
                        true,
                        "{ \"status\": \"pending\" }",
                    )],
                ),
                route(
                    "rt_charges_get",
                    "GET",
                    "/charges/:id",
                    "Get charge",
                    vec![resp("200 OK", 200, true, "{ \"status\": \"succeeded\" }")],
                ),
            ],
        }],
        routes: vec![route(
            "rt_refunds",
            "PUT",
            "/refunds/:id",
            "Update refund",
            vec![resp("200 OK", 200, true, "{}")],
        )],
    };
    Workspace {
        name: "example-workspace".into(),
        servers: vec![users, payments],
        environments: vec![
            Environment {
                name: "local".into(),
                vars: vec![
                    var("base_url", "http://127.0.0.1:4010", VarSource::File),
                    var("api_token", "tok_local_123456", VarSource::Dotenv),
                    var("region", "eu-west-1", VarSource::File),
                    var("HOME", "/Users/dev", VarSource::Process),
                ],
            },
            Environment {
                name: "staging".into(),
                vars: vec![
                    var("base_url", "https://staging.example.com", VarSource::File),
                    var("api_secret", "sec_staging_abcdef", VarSource::Dotenv),
                ],
            },
        ],
        datasets: vec!["users".into(), "charges".into()],
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrafficRow {
    pub time: String,
    pub server: String,
    pub method: &'static str,
    pub path: String,
    pub status: u16,
    pub route: String,
    pub duration_ms: f32,
    pub size: u32,
}

/// `n` deterministic rows (xorshift, fixed seed) shaped like real mocked traffic.
pub fn traffic(n: usize) -> Vec<TrafficRow> {
    const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];
    const STATUS: [u16; 8] = [200, 200, 200, 201, 204, 404, 422, 500];
    let mut s: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    (0..n)
        .map(|i| {
            let r = next();
            let method = METHODS[(r % 5) as usize];
            let id = (r >> 8) % 900 + 100;
            let secs = i % 86_400;
            let (server, path, route) = if (r >> 20) % 3 == 0 {
                ("Payments", format!("/charges/ch_{id}"), "Get charge")
            } else {
                ("Users API", format!("/api/users/{id}"), "Get user")
            };
            TrafficRow {
                time: format!(
                    "{:02}:{:02}:{:02}.{:03}",
                    secs / 3600,
                    secs / 60 % 60,
                    secs % 60,
                    (r >> 30) % 1000
                ),
                server: server.into(),
                method,
                path,
                status: STATUS[((r >> 40) % 8) as usize],
                route: route.into(),
                duration_ms: ((r >> 50) % 4000) as f32 / 100.0,
                size: ((r >> 12) % 9000) as u32 + 20,
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub warning: bool,
    pub message: String,
    /// Route to open when the problem is clicked.
    pub route_id: String,
}

pub fn problems() -> Vec<Problem> {
    vec![Problem {
        warning: true,
        message: "Payments: response \"200 OK\" of PUT /refunds/:id has an empty object body"
            .into(),
        route_id: "rt_refunds".into(),
    }]
}

pub fn logs() -> Vec<String> {
    [
        "10:02:11  INFO   host      workspace example-workspace loaded (2 servers, 8 routes)",
        "10:02:11  INFO   engine    Users API listening on http://127.0.0.1:4010",
        "10:02:12  WARN   validate  Payments: response \"200 OK\" of PUT /refunds/:id has an empty object body",
        "10:02:40  INFO   engine    GET /api/users/412 -> 200 (2.1 ms)",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_lookups() {
        let w = workspace();
        assert_eq!(w.running_count(), 1);
        let (s, r) = w.find_route("rt_users_get").unwrap();
        assert_eq!((s.port, r.method.as_str()), (4010, "GET"));
        assert!(w.find_route("nope").is_none());
        assert_eq!(w.find_environment("staging").unwrap().vars.len(), 2);
    }

    #[test]
    fn ids_are_unique() {
        let w = workspace();
        let mut ids: Vec<&str> = w
            .servers
            .iter()
            .flat_map(|s| s.all_routes().map(|r| r.id.as_str()))
            .collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn traffic_is_deterministic_and_sized() {
        let a = traffic(10_000);
        assert_eq!(a.len(), 10_000);
        assert_eq!(a, traffic(10_000));
        assert!(a.iter().any(|r| r.status == 500));
    }
}
