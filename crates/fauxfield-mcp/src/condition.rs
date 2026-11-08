//! A condition over a JSON document (`ui.state`): a JSON pointer plus one of `equals`,
//! `contains`, `matches`, optionally negated (SPEC §11.4). Pure, so `ui.wait_for` and the
//! scenario runner (M1.7) share it.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct Condition {
    /// JSON pointer into `ui.state`, e.g. `/command_palette/open`.
    pub pointer: String,
    /// The value at the pointer equals this JSON value.
    #[serde(default)]
    pub equals: Option<Value>,
    /// A string contains this substring, or an array contains this element.
    #[serde(default)]
    pub contains: Option<Value>,
    /// A string value matches this regular expression.
    #[serde(default)]
    pub matches: Option<String>,
    /// Invert the result.
    #[serde(default)]
    pub not: bool,
}

impl Condition {
    /// Evaluates against `doc`. A missing pointer is false (before `not`). An invalid regex, or
    /// a condition with no operator, is an error.
    pub fn eval(&self, doc: &Value) -> Result<bool, String> {
        let ops = [
            self.equals.is_some(),
            self.contains.is_some(),
            self.matches.is_some(),
        ];
        if ops.iter().filter(|b| **b).count() != 1 {
            return Err("condition needs exactly one of equals, contains, matches".into());
        }
        let found = doc.pointer(&self.pointer);
        let hit = match (found, &self.equals, &self.contains, &self.matches) {
            (None, ..) => false,
            (Some(v), Some(e), ..) => v == e,
            (Some(Value::String(s)), _, Some(Value::String(c)), _) => s.contains(c.as_str()),
            (Some(Value::Array(a)), _, Some(c), _) => a.contains(c),
            (Some(_), _, Some(_), _) => false,
            (Some(Value::String(s)), _, _, Some(re)) => regex::Regex::new(re)
                .map_err(|e| format!("invalid regex: {e}"))?
                .is_match(s),
            (Some(_), _, _, Some(re)) => {
                regex::Regex::new(re).map_err(|e| format!("invalid regex: {e}"))?;
                false
            }
            _ => false,
        };
        Ok(hit != self.not)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn c(v: Value) -> Condition {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn equals_contains_matches_and_not() {
        let doc = json!({"a": {"open": true, "q": "theme"}, "tabs": ["x", "y"]});
        assert!(
            c(json!({"pointer": "/a/open", "equals": true}))
                .eval(&doc)
                .unwrap()
        );
        assert!(
            !c(json!({"pointer": "/a/open", "equals": false}))
                .eval(&doc)
                .unwrap()
        );
        assert!(
            c(json!({"pointer": "/a/q", "contains": "hem"}))
                .eval(&doc)
                .unwrap()
        );
        assert!(
            c(json!({"pointer": "/tabs", "contains": "y"}))
                .eval(&doc)
                .unwrap()
        );
        assert!(
            c(json!({"pointer": "/a/q", "matches": "^th.me$"}))
                .eval(&doc)
                .unwrap()
        );
        assert!(
            c(json!({"pointer": "/a/q", "equals": "no", "not": true}))
                .eval(&doc)
                .unwrap()
        );
    }

    #[test]
    fn missing_pointer_is_false_and_errors_are_reported() {
        let doc = json!({});
        assert!(!c(json!({"pointer": "/x", "equals": 1})).eval(&doc).unwrap());
        assert!(
            c(json!({"pointer": "/x", "equals": 1, "not": true}))
                .eval(&doc)
                .unwrap()
        );
        assert!(c(json!({"pointer": "/x"})).eval(&doc).is_err());
        assert!(
            c(json!({"pointer": "/x", "matches": "("}))
                .eval(&json!({"x": "a"}))
                .is_err()
        );
    }
}
