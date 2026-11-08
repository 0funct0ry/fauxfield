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
        }
    }
}
