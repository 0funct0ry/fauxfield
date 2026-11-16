use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// A structured validation finding (SPEC §4.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Workspace-relative file, `/`-separated.
    pub file: String,
    /// Location inside the file, e.g. `routes[rte_x].responses[0].status`.
    pub path: String,
    pub severity: Severity,
    pub code: String,
    pub message: String,
    /// How to fix it (what to do next); filled from [`help`] for every known code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(
        file: &str,
        path: impl Into<String>,
        code: &str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(file, path, Severity::Error, code, message)
    }
    pub fn warning(
        file: &str,
        path: impl Into<String>,
        code: &str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(file, path, Severity::Warning, code, message)
    }
    fn new(
        file: &str,
        path: impl Into<String>,
        severity: Severity,
        code: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            file: file.into(),
            path: path.into(),
            severity,
            code: code.into(),
            message: message.into(),
            help: help(code).map(str::to_string),
        }
    }
}

/// The help line for a diagnostic code: what to do about it (SPEC §20.34). `None` for an
/// unknown code; a test requires one for every code the validator emits.
pub fn help(code: &str) -> Option<&'static str> {
    Some(match code {
        "E001" => "Fix the TOML syntax at the reported line; the file is skipped until it parses.",
        "E002" => {
            "Set `schema` to the version this Fauxfield writes, or update Fauxfield to open newer workspaces."
        }
        "E010" => "Give each Server its own `id`; rename one of the duplicates.",
        "E011" => "Give each Route its own `id` inside the Server; rename one of the duplicates.",
        "E012" => "Give each Folder its own `id` inside the Server; rename one of the duplicates.",
        "E013" => "Give each Response its own `id` inside the Route; rename one of the duplicates.",
        "E020" => {
            "Choose a free port in the Server settings, or stop the other Server that uses it."
        }
        "E021" => "Set a port from 1 to 65535 in the Server settings.",
        "E030" => {
            "Point `folder` at an existing Folder id, or remove it to put the Route at the top level."
        }
        "E031" => "Start the path with `/`, for example `/users/{id}`.",
        "E032" => "Use one of GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS or ANY.",
        "E033" => {
            "Check the `{name}` and `*` placeholders in the path pattern; each placeholder needs a name and no repeats."
        }
        "E034" => "Set `match` to `exact`, `pattern` or `regex`.",
        "E035" => "Fix the regular expression; test it in a regex tool, then paste it back.",
        "E040" => "Set a status from 100 to 599.",
        "E041" => "Keep either `body` or `body_file` on the Response and delete the other.",
        "E042" => {
            "Use a path inside the Workspace folder; `..` and absolute paths outside it are not allowed."
        }
        "E043" => {
            "Set `response_mode` to `default`, `sequence`, `random` or `weighted`, or remove it."
        }
        "E044" => {
            "Pick a Rule target from the list in the Rule editor (path, query, header, body, cookie, dataset)."
        }
        "E045" => "Pick a Rule operator from the list in the Rule editor.",
        "E046" => "Fill in the key for this Rule target, for example the header name.",
        "E047" => "Write the regex value as a string.",
        "E048" => "Fix the JSONPath in the Rule key; it must start with `$`.",
        "E049" => "Set `rules_logic` to `and` or `or`.",
        "E060" => "Give each Environment its own name; rename one of the duplicates.",
        "E070" => "Set the Route `kind` to `mock`, `crud` or `static`.",
        "E071" => "Choose the Dataset a crud Route serves in the Route editor.",
        "E072" => "Pick a Dataset that exists in this Server, or create it in the Datasets tab.",
        "E073" => "Give each Dataset a unique, non-empty name.",
        "E074" => "Set `id_strategy` to `uuid`, `increment` or `ulid`.",
        "E075" => "Keep only one of `seed`, `seed_template` and `generator` on the Dataset.",
        "E076" => "Fix the template error in the seed; the message shows the line and column.",
        "E077" => "Break the cycle by removing one of the generator `ref` fields.",
        "E078" => "Give each column a unique, non-empty name.",
        "E079" => "Choose a column type from string, int, float, bool, datetime or json.",
        "E080" => "Set requests to 1 or more in the rate limit settings.",
        "E081" => "Write the window like `30s`, `1m` or `2h`.",
        "E082" => "Use `ip`, `header:<name>` or `query:<name>` as the rate limit key.",
        "E083" => "Write the delay as milliseconds (`300`) or a range (`200..800`).",
        "E084" => "Pick a fault from the Response fault list.",
        "E085" => "Set `fault_rate` between 0 and 1.",
        "E086" => "Point the action at a Dataset that exists in this Server.",
        "E087" => "Fix the action definition; the message names the field.",
        "E088" => "Point the Rule at a Dataset that exists in this Server.",
        "E090" => "Set `dir` to the folder to serve, relative to the Workspace.",
        "E091" => "Start each template extension with a dot, for example `.tera`.",
        "E092" => "Set the TLS mode to `dev-ca` or `files`.",
        "E093" => "Set both `cert` and `key` when the TLS mode is `files`.",
        "E094" => "Choose an `also_http_port` that differs from the Server port.",
        "E095" => "Set the proxy mode to `fallback` or `passthrough`.",
        "E096" => "Enter the upstream URL, or turn the proxy off.",
        "E097" => "Write the upstream as a full URL starting with http:// or https://.",
        "E098" => "Mark only one column as the primary key.",
        "E099" => "Change the column default so it fits the column type.",
        "E100" => "Make the primary key column match the Dataset `key`, or change the `key`.",
        "E101" => "Use only list, get, create, update, patch and delete in `crud_ops`.",
        "E116" => "Use only list, get, create, update, patch and delete as operation names.",
        "E117" => "Add the operation to `crud_ops`, or remove its Responses.",
        "E102" => "Set the WebSocket mode to `echo`, `script` or `broadcast`.",
        "E103" => "Set the trigger to `connect`, `interval` or `message`.",
        "E104" => "Write `every` like `500ms`, `5s` or `1m`; the minimum is 10ms.",
        "E105" => "Encode the binary message body as base64.",
        "E106" => "Use a close code from 1000 to 4999.",
        "E107" => "Set the message format to `compact`, `pretty` or `raw`.",
        "E108" => "Give each Callback a unique id inside its Response.",
        "E109" => "Enter the URL the Callback should call.",
        "E110" => "Write the Callback URL starting with http:// or https://.",
        "E111" => "Use a standard HTTP method for the Callback.",
        "E112" => "Lower `retries` to the allowed maximum.",
        "W042" => {
            "Create the file, fix the path, or remove `body_file`; the Response sends an empty body meanwhile."
        }
        "W043" => "Keep one default Response; clear `default` on the others.",
        "W044" => "Enable the default Response, or choose another default.",
        "W050" => "Rename the Server id to start with `srv_` (optional; this is a convention).",
        "W051" => "Move the Route above the one that shadows it, or make its path more specific.",
        "W052" => "Switch `match` to `pattern`, or ignore this if the path really contains braces.",
        "W060" => "Set `bandwidth_kbps` on the Server or a Network profile, or pick another fault.",
        "W070" => {
            "Remove the Responses, change the Route `kind` to `http`, or on a crud Route move them under an operation."
        }
        "W073" => {
            "Add a Rule to the Response, or delete it; a Response without Rules is never chosen."
        }
        "W071" => "Create the seed file, or fix its path; the Dataset starts empty until then.",
        "W072" => "Be aware that the action changes the Dataset file on disk.",
        "W098" => {
            "Remove `default` from the Response; an operation falls through to the built-in handler instead."
        }
        "W090" => "List the allowed origins in CORS settings, or turn CORS off.",
        "W091" => {
            "Turn off accepting invalid certificates before using the proxy with real services."
        }
        "W092" => "Set the Route to an http Route, or remove `proxy` and `pinned`.",
        "W093" => "Remove `when`, or change the trigger to `message`.",
        "W094" => "Pick a fault that keeps the connection open, or remove the Callbacks.",
        "E113" => {
            "Use a path like `/__docs`: starts with `/`, no trailing slash, and no `:`, `*` or `{`."
        }
        "E114" => {
            "Use a unique operation_id without spaces, and an openapi.path that starts with `/`."
        }
        "E115" => "Point the schema at a workspace file containing a valid JSON Schema.",
        "W095" => {
            "Bind the Server to 127.0.0.1, or turn off API documentation; anyone on the network can read it."
        }
        "W096" => "Move the Route away from the docs path, or change `docs_site.path`.",
        "W097" => {
            "Give the Route an `openapi.path`, or set `openapi.hidden = true` to silence this."
        }
        _ => return None,
    })
}

/// Every code with a help line, sorted; the generated diagnostics reference reads this.
pub const CODES: &[&str] = &[
    "E001", "E002", "E010", "E011", "E012", "E013", "E020", "E021", "E030", "E031", "E032", "E033",
    "E034", "E035", "E040", "E041", "E042", "E043", "E044", "E045", "E046", "E047", "E048", "E049",
    "E060", "E070", "E071", "E072", "E073", "E074", "E075", "E076", "E077", "E078", "E079", "E080",
    "E081", "E082", "E083", "E084", "E085", "E086", "E087", "E088", "E090", "E091", "E092", "E093",
    "E094", "E095", "E096", "E097", "E098", "E099", "E100", "E101", "E102", "E103", "E104", "E105",
    "E106", "E107", "E108", "E109", "E110", "E111", "E112", "E113", "E114", "E115", "E116", "E117",
    "W042", "W043", "W044", "W050", "W051", "W052", "W060", "W070", "W071", "W072", "W073", "W090",
    "W091", "W092", "W093", "W094", "W095", "W096", "W097", "W098",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_help() {
        for c in CODES {
            assert!(help(c).is_some(), "{c}");
        }
    }

    /// Every quoted diagnostic code in the crates sources has a help line.
    #[test]
    fn every_emitted_code_has_help() {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    let b = text.as_bytes();
                    for i in 0..b.len().saturating_sub(5) {
                        if b[i] == b'"'
                            && (b[i + 1] == b'E' || b[i + 1] == b'W')
                            && b[i + 2..i + 5].iter().all(u8::is_ascii_digit)
                            && b[i + 5] == b'"'
                        {
                            out.push(text[i + 1..i + 5].to_string());
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../"),
            &mut found,
        );
        assert!(found.len() > 50);
        for c in found {
            assert!(c == "E999" || help(&c).is_some(), "{c} has no help line");
        }
    }

    #[test]
    fn constructor_fills_help() {
        let d = Diagnostic::error("a.toml", "x", "E020", "port in use");
        assert!(d.help.as_deref().unwrap().contains("free port"));
        assert!(Diagnostic::error("a", "x", "E999", "m").help.is_none());
    }
}
