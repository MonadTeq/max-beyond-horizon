//! SHA-256 helpers shared by the manifest and the encoder.

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of `bytes` (64 characters).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);

    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn sha256_matches_the_known_empty_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_is_lowercase_hex_of_fixed_length() {
        let hex = sha256_hex(b"M.A.X.");

        assert_eq!(hex.len(), 64);
        assert!(hex
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
