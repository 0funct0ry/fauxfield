//! Traffic rows (runtime-only, never files). Real rows come from engine recordings; the
//! synthetic generator below exists only in test and dev-mcp builds (screenshots and the scroll
//! scenarios), so a release build does not contain it.

use fauxfield_engine::Recording;

#[derive(Clone, Debug, PartialEq)]
pub struct TrafficRow {
    /// Engine recording ID; 0 for synthetic rows.
    pub id: u64,
    /// The full recording, for the Inspector detail and Copy as cURL.
    pub rec: Option<Box<Recording>>,
    pub time: String,
    pub server: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub route: String,
    pub duration_ms: f32,
    pub size: u32,
}

impl TrafficRow {
    pub fn from_recording(r: &Recording) -> TrafficRow {
        TrafficRow {
            id: r.id,
            time: r.ts.get(11..23).unwrap_or(&r.ts).to_string(),
            server: r.server.clone(),
            method: r.method.clone(),
            path: r.url.clone(),
            status: r.status,
            route: r.route.clone(),
            duration_ms: r.timing.total_ms as f32,
            size: r.response_body.len() as u32,
            rec: Some(Box::new(r.clone())),
        }
    }
}

/// `n` deterministic rows (xorshift, fixed seed) shaped like real mocked traffic.
#[cfg(any(test, feature = "dev-mcp"))]
pub fn synthetic(n: usize) -> Vec<TrafficRow> {
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
                id: 0,
                rec: None,
                time: format!(
                    "{:02}:{:02}:{:02}.{:03}",
                    secs / 3600,
                    secs / 60 % 60,
                    secs % 60,
                    (r >> 30) % 1000
                ),
                server: server.into(),
                method: method.to_string(),
                path,
                status: STATUS[((r >> 40) % 8) as usize],
                route: route.into(),
                duration_ms: ((r >> 50) % 4000) as f32 / 100.0,
                size: ((r >> 12) % 9000) as u32 + 20,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_traffic_is_deterministic_and_sized() {
        let a = synthetic(10_000);
        assert_eq!(a.len(), 10_000);
        assert_eq!(a, synthetic(10_000));
        assert!(a.iter().any(|r| r.status == 500));
    }
}
