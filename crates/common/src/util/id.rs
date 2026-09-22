//! Random identifiers backed by `/dev/urandom` (avoids a uuid-crate dependency).

use std::fs::File;
use std::io::Read;

/// Read `n` random bytes as a lowercase hex string. Falls back to a
/// process/time-seeded value if `/dev/urandom` is unavailable.
pub fn random_hex(n: usize) -> String {
    let mut buf = vec![0u8; n];
    if File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_err()
    {
        // Extremely unlikely on Linux; seed from pid + nanos so ids stay unique.
        let seed = std::process::id() as u128
            ^ std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = (seed >> (8 * (i % 16))) as u8;
        }
    }
    hex::encode(buf)
}

/// A random UUIDv4 string (`xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`).
pub fn random_uuid() -> String {
    let mut bytes = [0u8; 16];
    let hex = random_hex(16);
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant
    let h = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Crockford Base32 (ULID alphabet). No I, L, O, U.
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A 26-character ULID. Time-sortable, generated from `/dev/urandom` like
/// [`random_uuid`] — no extra crate.
pub fn random_ulid() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
        & 0x0000_FFFF_FFFF_FFFF;
    let entropy = random_hex(10);
    let mut n: u128 = (ms as u128) << 80;
    for i in 0..10 {
        let byte = u8::from_str_radix(&entropy[i * 2..i * 2 + 2], 16).unwrap_or(0);
        n |= (byte as u128) << (8 * (9 - i));
    }
    let mut out = String::with_capacity(26);
    for i in (0..26).rev() {
        let idx = ((n >> (5 * i)) & 31) as usize;
        out.push(CROCKFORD[idx] as char);
    }
    out
}

/// True iff `s` is a 26-character Crockford ULID (the only legal inbox draft id).
pub fn is_ulid(s: &str) -> bool {
    s.len() == 26
        && s.bytes()
            .all(|b| CROCKFORD.contains(&b.to_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_ulid_is_crockford_26() {
        let id = random_ulid();
        assert!(is_ulid(&id), "{id}");
        let again = random_ulid();
        assert_ne!(id, again);
    }

    #[test]
    fn is_ulid_rejects_path_and_wrong_length() {
        assert!(!is_ulid("../secret"));
        assert!(!is_ulid("short"));
        assert!(is_ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
    }
}
