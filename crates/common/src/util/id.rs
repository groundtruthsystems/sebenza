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
