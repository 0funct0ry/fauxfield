const ALPHABET: &[u8; 32] = b"abcdefghijkmnpqrstuvwxyz23456789";

/// A short prefixed random ID, e.g. `srv_7h2kq9xa` (SPEC §4.5).
pub fn new_id(prefix: &str) -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("os randomness");
    let mut id = String::with_capacity(prefix.len() + 9);
    id.push_str(prefix);
    id.push('_');
    id.extend(bytes.iter().map(|b| ALPHABET[(b & 31) as usize] as char));
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_prefixed_and_distinct() {
        let a = new_id("srv");
        assert!(a.starts_with("srv_") && a.len() == 12);
        assert_ne!(a, new_id("srv"));
    }
}
