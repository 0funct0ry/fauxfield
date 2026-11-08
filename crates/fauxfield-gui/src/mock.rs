//! Mock Traffic and Logs until the engine arrives in M3 (runtime-only data, never files).

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
    fn traffic_is_deterministic_and_sized() {
        let a = traffic(10_000);
        assert_eq!(a.len(), 10_000);
        assert_eq!(a, traffic(10_000));
        assert!(a.iter().any(|r| r.status == 500));
    }
}
