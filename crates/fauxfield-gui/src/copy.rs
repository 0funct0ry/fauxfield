//! The English copy catalog (SPEC §9.10, §20.34). Empty-state, toast and error text lives here
//! so views carry no string literals for it and a later translation has one place to start.
//! Files listed in `scripts/lint-copy.files` are linted for literals in their view code.

/// One empty state: what is missing, what to do next, and where the primary button leads.
pub struct EmptySpec {
    /// Stable id; the element id is `empty-<id>` and `ui.state` lists it.
    pub id: &'static str,
    pub icon: &'static str,
    pub title: &'static str,
    pub hint: &'static str,
    /// Primary button: label and the action id it runs (its live binding is shown beside it).
    pub primary: Option<(&'static str, &'static str)>,
    /// Secondary links: label and action id.
    pub links: &'static [(&'static str, &'static str)],
}

macro_rules! empty {
    ($id:literal, $icon:literal, $title:literal, $hint:literal, $primary:expr, $links:expr) => {
        EmptySpec {
            id: $id,
            icon: $icon,
            title: $title,
            hint: $hint,
            primary: $primary,
            links: $links,
        }
    };
}

pub const EMPTY: &[EmptySpec] = &[
    empty!(
        "servers",
        "server",
        "No Servers yet",
        "Create a Server to start mocking an API.",
        Some(("New Server…", "new_server")),
        &[("Open a Workspace…", "open_folder")]
    ),
    empty!(
        "routes",
        "list-tree",
        "No Routes yet",
        "Add a Route to answer requests on this Server.",
        Some(("New Route…", "new_route")),
        &[("New Folder…", "new_folder")]
    ),
    empty!(
        "folder-routes",
        "folder",
        "This Folder is empty",
        "Drag Routes here or add a new one.",
        Some(("New Route…", "new_route")),
        &[]
    ),
    empty!(
        "responses",
        "file-text",
        "No Responses yet",
        "A Route needs a Response to answer with.",
        Some(("New Response", "new_response")),
        &[]
    ),
    empty!(
        "rules",
        "braces",
        "No Rules",
        "Without Rules the default Response answers every request.",
        None,
        &[]
    ),
    empty!(
        "callbacks",
        "zap",
        "No Callbacks",
        "A Callback sends a webhook after the Response; add one with +.",
        None,
        &[]
    ),
    empty!(
        "environments",
        "layers",
        "No Environments",
        "An Environment holds Variables such as a base URL.",
        Some(("New Environment…", "new_environment")),
        &[]
    ),
    empty!(
        "variables",
        "braces",
        "No Variables",
        "Add a Variable and use it in a Response as {{ name }}.",
        None,
        &[]
    ),
    empty!(
        "datasets",
        "database",
        "No Datasets",
        "A Dataset holds the rows a CRUD Route serves.",
        Some(("New Dataset…", "new_dataset")),
        &[]
    ),
    empty!(
        "dataset-rows",
        "table",
        "No rows yet",
        "Add a row, import a file or generate seed data.",
        Some(("Add Row", "add_dataset_row")),
        &[]
    ),
    empty!(
        "dataset-rows-filtered",
        "search",
        "No rows match the filter",
        "Clear the filter to see every row.",
        None,
        &[]
    ),
    empty!(
        "traffic-stopped",
        "play",
        "No requests yet",
        "Start a Server, then send a request.",
        Some(("Run All Servers", "run_all")),
        &[]
    ),
    empty!(
        "traffic",
        "terminal",
        "No requests yet",
        "Send one with curl, or send a test request from a Route.",
        Some(("New Route…", "new_route")),
        &[]
    ),
    empty!(
        "problems",
        "check",
        "No problems",
        "Every file in the Workspace is valid.",
        None,
        &[]
    ),
    empty!(
        "logs",
        "file-text",
        "No log lines yet",
        "Servers log when they start, stop and handle requests.",
        Some(("Run All Servers", "run_all")),
        &[]
    ),
    empty!(
        "agent-activity",
        "terminal",
        "No agent activity yet",
        "Tool calls from a connected agent appear here.",
        None,
        &[]
    ),
    empty!(
        "search-no-results",
        "search",
        "No results",
        "Try fewer words or another spelling.",
        None,
        &[]
    ),
    empty!(
        "palette-no-results",
        "search",
        "No matching command",
        "Try a shorter word, for example \"server\".",
        None,
        &[]
    ),
];

pub fn empty(id: &str) -> &'static EmptySpec {
    EMPTY
        .iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("no empty state `{id}` in the copy catalog"))
}

/// "Title. Hint" on one line, for compact lists that have no room for the full block.
pub fn empty_line(id: &str) -> String {
    let e = empty(id);
    format!("{}. {}", e.title, e.hint)
}

/// Toasts: past-tense verb plus noun (SPEC §20.34).
pub const TOAST_MAX: usize = 3;
pub const TOAST_SECS: u64 = 5;
pub const COPY_DETAILS: &str = "Copy details";
pub const UNDO: &str = "Undo";

/// Confirm labels that destroy something draw a red button and leave Cancel as the default.
pub fn is_destructive(label: &str) -> bool {
    [
        "Delete",
        "Discard",
        "Remove",
        "Clear",
        "Reset",
        "Overwrite",
        "Quit",
    ]
    .iter()
    .any(|v| label.starts_with(v))
}

/// Fills `{}` placeholders in order.
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut it = args.iter();
    let mut rest = template;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(it.next().copied().unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// What happened, why, how to fix (SPEC §20.34).
pub const ERR_PORT_IN_USE: &str = "Couldn’t start “{}”. Port {} is already in use by another process. Choose a free port in Server settings.";
pub const ERR_SERVER_DISABLED: &str =
    "Couldn’t start “{}”. The Server is disabled in its file. Enable it, then start it again.";
pub const ERR_SERVER_START: &str =
    "Couldn’t start “{}”. {} Check the Server settings and try again.";

/// The state-fixture ids for each empty state (UX140): the `ui.state` fixture that shows it.
pub fn fixture_of(id: &str) -> Option<&'static str> {
    Some(match id {
        "servers" => "sidebar-servers-empty",
        "environments" => "sidebar-environments-empty",
        "datasets" => "sidebar-datasets-empty",
        "traffic" | "traffic-stopped" => "traffic-empty",
        "problems" => "problems-empty",
        "search-no-results" => "quick-search-empty",
        "palette-no-results" => "palette-empty",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_actions_exist() {
        let mut seen = std::collections::HashSet::new();
        for e in EMPTY {
            assert!(seen.insert(e.id), "{}", e.id);
            for (_, a) in e.primary.iter().chain(e.links.iter()) {
                assert!(crate::actions::find(a).is_some(), "{} -> {a}", e.id);
            }
        }
    }

    #[test]
    fn destructive_labels() {
        assert!(is_destructive("Delete"));
        assert!(is_destructive("Discard changes"));
        assert!(!is_destructive("Create"));
    }

    #[test]
    fn fill_replaces_in_order() {
        assert_eq!(fill("a {} b {} c", &["1", "2"]), "a 1 b 2 c");
        assert_eq!(fill("x {}", &[]), "x ");
    }

    #[test]
    fn error_copy_has_what_why_fix() {
        let m = fill(ERR_PORT_IN_USE, &["Todo API", "4010"]);
        assert!(m.starts_with("Couldn’t start “Todo API”."));
        assert!(m.contains("already in use") && m.contains("Choose a free port"));
    }
}
