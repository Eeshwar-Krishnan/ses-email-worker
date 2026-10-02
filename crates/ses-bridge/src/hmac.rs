use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// HMAC material attached to an outbound bridge request.
pub struct Signature {
    pub timestamp: String,
    pub nonce: String,
    pub signature: String,
}

/// Signs `method path timestamp nonce sha256(body)` with HMAC-SHA256.
///
/// The Worker reconstructs the same canonical string from the raw request,
/// so the exact bytes sent on the wire must be hashed here.
pub fn sign(secret: &str, method: &str, path: &str, body: &[u8]) -> Signature {
    let timestamp = now_secs().to_string();
    let nonce = random_hex(16);
    let body_hash = hex::encode(Sha256::digest(body));
    let canonical = format!("{method}\n{path}\n{timestamp}\n{nonce}\n{body_hash}");

    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(canonical.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    Signature {
        timestamp,
        nonce,
        signature,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("failed to read OS randomness");
    hex::encode(buf)
}
