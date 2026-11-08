//! Tool classes (SPEC §11.1): every tool belongs to exactly one.

use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolClass {
    Read,
    Write,
    Execute,
    /// Only in `dev-mcp` builds (SPEC §11.4); never allowed by `--allow`.
    Ui,
}

impl ToolClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Execute => "execute",
            Self::Ui => "ui",
        }
    }
    fn bit(self) -> u8 {
        match self {
            Self::Read => 1,
            Self::Write => 2,
            Self::Execute => 4,
            Self::Ui => 8,
        }
    }
}

impl fmt::Display for ToolClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ToolClass {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "execute" => Ok(Self::Execute),
            "ui" => Err("the `ui` class is only available in dev-mcp builds".into()),
            other => Err(format!(
                "unknown tool class `{other}` (expected read, write or execute)"
            )),
        }
    }
}

/// The set of classes a server instance exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowedClasses(u8);

impl AllowedClasses {
    pub fn all() -> Self {
        Self(ToolClass::Read.bit() | ToolClass::Write.bit() | ToolClass::Execute.bit())
    }
    pub fn read_only() -> Self {
        Self(ToolClass::Read.bit())
    }
    pub fn from_classes(classes: impl IntoIterator<Item = ToolClass>) -> Self {
        Self(classes.into_iter().fold(0, |a, c| a | c.bit()))
    }
    /// Parse a comma list such as `read,write`.
    pub fn parse_list(list: &str) -> Result<Self, String> {
        let mut v = Vec::new();
        for part in list.split(',').filter(|p| !p.trim().is_empty()) {
            v.push(part.parse::<ToolClass>()?);
        }
        if v.is_empty() {
            return Err("at least one tool class is required".into());
        }
        Ok(Self::from_classes(v))
    }
    pub fn with(self, c: ToolClass) -> Self {
        Self(self.0 | c.bit())
    }
    pub fn contains(self, c: ToolClass) -> bool {
        self.0 & c.bit() != 0
    }
    pub fn names(self) -> Vec<&'static str> {
        [
            ToolClass::Read,
            ToolClass::Write,
            ToolClass::Execute,
            ToolClass::Ui,
        ]
        .into_iter()
        .filter(|c| self.contains(*c))
        .map(ToolClass::as_str)
        .collect()
    }
}

impl Default for AllowedClasses {
    fn default() -> Self {
        Self::all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_filters() {
        let a = AllowedClasses::parse_list("read, execute").unwrap();
        assert!(a.contains(ToolClass::Read) && a.contains(ToolClass::Execute));
        assert!(!a.contains(ToolClass::Write) && !a.contains(ToolClass::Ui));
        assert_eq!(a.names(), ["read", "execute"]);
        assert!(AllowedClasses::parse_list("bogus").is_err());
        assert!(AllowedClasses::parse_list("ui").is_err());
        assert!(AllowedClasses::parse_list("").is_err());
    }
}
