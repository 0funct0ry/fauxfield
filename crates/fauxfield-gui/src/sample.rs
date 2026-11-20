//! The sample request the template preview renders against.

use crate::view::{Route, Server};
use fauxfield_engine::TestRequest;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// Empty: the Route's own method.
    pub method: String,
    /// `a=1&b=2`, without the leading `?`.
    pub query: String,
    /// `Name: value` per line.
    pub headers: String,
    /// `a=1; b=2`.
    pub cookies: String,
    pub body: String,
}

/// The headers a typical client sends, so templates that read them preview without errors.
/// A header outside this list is missing from the preview, as it would be for a client that
/// omits it (use `| default(value=...)`).
pub const SAMPLE_HEADERS: &str = "accept: application/json\n\
content-type: application/json\n\
user-agent: fauxfield-preview\n\
authorization: Bearer sample-token\n\
x-request-id: 3f2b8c1e-5d4a-4e6f-9a7b-0c1d2e3f4a5b";

impl Default for Sample {
    fn default() -> Self {
        Sample {
            method: String::new(),
            query: String::new(),
            headers: SAMPLE_HEADERS.to_string(),
            cookies: String::new(),
            body: String::new(),
        }
    }
}

impl Sample {
    /// The request the template preview renders for `route` on `server`.
    pub fn request(&self, server: &Server, route: &Route) -> TestRequest {
        let base = if route.method.eq_ignore_ascii_case("any") {
            "GET"
        } else {
            &route.method
        };
        let method = if self.method.trim().is_empty() {
            base.to_string()
        } else {
            self.method.trim().to_ascii_uppercase()
        };
        let mut path = format!(
            "{}/{}",
            server.prefix.trim_end_matches('/'),
            route.path.trim_start_matches('/')
        );
        let q = self.query.trim().trim_start_matches('?');
        if !q.is_empty() {
            path.push('?');
            path.push_str(q);
        }
        let mut headers: Vec<(String, String)> = self
            .headers
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .filter(|(k, _)| !k.is_empty())
            .collect();
        if !self.cookies.trim().is_empty() {
            headers.push(("cookie".into(), self.cookies.trim().to_string()));
        }
        TestRequest {
            method,
            path,
            headers,
            body: self.body.clone(),
        }
    }
}
