//! Admin token scheme (shared with the `max_admin` WebSocket plugin).
//!
//! A token is `hex(HMAC-SHA256(admin_secret, "max-admin"))`, exactly as the WS
//! plugin computes it (`crates/max_admin`, `docs/protocol.md` §5.4), so an
//! operator can use the same token on both transports. Verification is
//! constant-time (`hmac`'s `verify_slice`) and fail-closed: without a configured
//! secret every admin RPC is rejected.
//!
//! The implementation is intentionally duplicated from `max_admin` instead of
//! depending on it: that crate is a `cdylib` plugin and the adapter must not
//! change how plugins are built. The known-answer test below pins the algorithm
//! to the value `max_admin` produces.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

/// HMAC-SHA256 used for admin tokens.
type HmacSha256 = Hmac<Sha256>;

/// Block size of SHA-256 in bytes; HMAC keys longer than this are hashed down
/// first (RFC 2104), which keeps `HmacSha256::new_from_slice` infallible.
const SHA256_BLOCK_LEN: usize = 64;

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Mints the admin token for `secret`: lowercase hex of
/// `HMAC-SHA256(secret, "max-admin")`.
///
/// This is the operator-facing half of the scheme: tooling computes the token
/// with this function (or any HMAC-SHA256 implementation) and sends it in the
/// `admin_token` field of an admin RPC.
pub fn admin_token_for(secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(&effective_hmac_key(secret.as_bytes()))
        .expect("effective_hmac_key never exceeds the SHA-256 block size");
    mac.update(b"max-admin");
    encode_hex(&mac.finalize().into_bytes())
}

/// Constant-time check of a presented admin token.
pub fn verify_admin_token(secret: &str, token: &str) -> bool {
    let Some(expected) = decode_hex(token) else {
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(&effective_hmac_key(secret.as_bytes()))
        .expect("effective_hmac_key never exceeds the SHA-256 block size");
    mac.update(b"max-admin");
    mac.verify_slice(&expected).is_ok()
}

/// The effective HMAC key: keys longer than the SHA-256 block size are hashed
/// down (RFC 2104 step 1), so `HmacSha256::new_from_slice` never fails.
fn effective_hmac_key(secret: &[u8]) -> Vec<u8> {
    if secret.len() > SHA256_BLOCK_LEN {
        let mut hasher = Sha256::new();
        hasher.update(secret);
        hasher.finalize().to_vec()
    } else {
        secret.to_vec()
    }
}

/// Lowercase hex encoding.
fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        out.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Checked hex decoding; accepts upper- and lowercase digits, rejects odd lengths.
fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    let bytes = hex.as_bytes();
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        out.push((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?);
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_lowercase_hex_hmac_sha256() {
        let token = admin_token_for("secret");

        // Known-answer test: HMAC-SHA256("secret", "max-admin"), i.e. exactly the
        // token crates/max_admin hands out for the same secret.
        assert_eq!(
            token,
            "66ee0d43d200c93d828c92bc76a4949b84b3619665d3d056cd75fcd56ebc6405"
        );
        assert_eq!(token.len(), 64);
        assert!(token
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
    }

    #[test]
    fn verification_accepts_only_the_right_token() {
        let secret = "admin-unit-test-secret";
        let token = admin_token_for(secret);

        assert_eq!(
            token,
            "ca22bf2a6e77c3c0d5c22fb5a0ceb09fc9e94f6266a05c3f700cfee979d9b012"
        );
        assert!(verify_admin_token(secret, &token));
        assert!(verify_admin_token(secret, &token.to_ascii_uppercase()));

        assert!(!verify_admin_token(secret, "not-the-token"));
        assert!(!verify_admin_token(secret, ""));
        assert!(!verify_admin_token("another-secret", &token));
        assert!(!verify_admin_token(secret, "not-hex"));
    }

    #[test]
    fn long_secrets_are_hashed_down_and_still_work() {
        let long_secret: String = (0..1024).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        let token = admin_token_for(&long_secret);

        assert!(verify_admin_token(&long_secret, &token));
        assert!(!verify_admin_token(&long_secret[..64], &token));
    }
}
