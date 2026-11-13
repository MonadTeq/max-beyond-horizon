//! Session ticket issuing and verification.
//!
//! A ticket is a plain ASCII string, safe to put into game configuration files, that a
//! client presents during the ENet transport handshake (`TRANSPORT_PACKET_00`):
//!
//! ```text
//! v1.<session_id>.<player_id>.<unix_expiry>.<hex_hmac>
//! ```
//!
//! * `session_id` and `player_id` are hyphenated, lowercase UUIDs;
//! * `unix_expiry` is the expiration time in whole Unix seconds;
//! * `hex_hmac` is the lowercase hex encoding of an HMAC-SHA256 over the string
//!   `v1.<session_id>.<player_id>.<unix_expiry>` keyed with the shared secret.
//!
//! Verification is fuzz-safe: every parsing step is a checked operation, [`Auth::verify`]
//! never panics on arbitrary input, and the signature is compared in constant time
//! (`Mac::verify_slice` from the `hmac` crate).

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// HMAC-SHA256 used for ticket signatures.
type HmacSha256 = Hmac<Sha256>;

/// Block size of SHA-256 in bytes. HMAC keys longer than this are hashed down first, as
/// RFC 2104 prescribes (and because `HmacSha256::new_from_slice` rejects longer keys).
const SHA256_BLOCK_LEN: usize = 64;

/// Length of an HMAC-SHA256 output in bytes.
const MAC_LEN: usize = 32;

/// The only supported ticket version.
const VERSION: &str = "v1";

/// Shared-secret ticket authority.
///
/// One `Auth` instance corresponds to one server: the same secret must be used to issue and
/// to verify tickets. The secret is deliberately excluded from `Debug` output.
pub struct Auth {
    secret: Vec<u8>,
    /// Wall clock used for expiry computation; a plain function pointer so tests can
    /// inject a fake clock.
    clock: fn() -> SystemTime,
}

impl Auth {
    /// Creates a ticket authority from a shared secret.
    ///
    /// An empty secret is accepted so an unconfigured lobby can still talk to `max_auth`,
    /// but it makes every ticket forgeable — use a real secret in production.
    pub fn new(secret: impl Into<Vec<u8>>) -> Self {
        Self {
            secret: secret.into(),
            clock: SystemTime::now,
        }
    }

    /// Issues a signed ticket for `player_id` joining `session_id`, valid for `ttl`.
    ///
    /// The expiry is whole Unix seconds of `now + ttl`; the returned string never contains
    /// the secret and is safe to hand to clients and to put into game settings.
    pub fn issue(&self, session_id: Uuid, player_id: Uuid, ttl: Duration) -> String {
        let expires_at = self
            .unix_now()
            .saturating_add(ttl.as_secs().min(i64::MAX as u64) as i64);
        let body = format!("{VERSION}.{session_id}.{player_id}.{expires_at}");
        let mac = self.sign(body.as_bytes());
        format!("{body}.{mac}")
    }

    /// Verifies a ticket presented for `expected_session_id`.
    ///
    /// Checks, in order: the wire format ([`VerifyError::Malformed`]), the signature
    /// (constant time, [`VerifyError::BadSignature`]), the session
    /// ([`VerifyError::WrongSession`]) and the expiry ([`VerifyError::Expired`]). A ticket
    /// is expired as soon as `now >= expires_at_unix`. Never panics on arbitrary input.
    pub fn verify(&self, ticket: &str, expected_session_id: Uuid) -> Result<Ticket, VerifyError> {
        let (body, mac_hex) = ticket.rsplit_once('.').ok_or(VerifyError::Malformed)?;
        let parsed = ParsedTicket::parse(body, mac_hex)?;

        if !self.verify_mac(body.as_bytes(), &parsed.mac) {
            return Err(VerifyError::BadSignature);
        }
        if parsed.session_id != expected_session_id {
            return Err(VerifyError::WrongSession);
        }
        if self.unix_now() >= parsed.expires_at_unix {
            return Err(VerifyError::Expired);
        }

        Ok(Ticket {
            session_id: parsed.session_id,
            player_id: parsed.player_id,
            expires_at_unix: parsed.expires_at_unix,
        })
    }

    /// Current wall-clock time in whole Unix seconds.
    fn unix_now(&self) -> i64 {
        unix_seconds((self.clock)())
    }

    /// The effective HMAC key: keys longer than the SHA-256 block size are hashed down
    /// (RFC 2104 step 1). This guarantees `HmacSha256::new_from_slice` never fails, which
    /// keeps both `sign` and `verify_mac` panic-free for any secret.
    fn effective_key(&self) -> Vec<u8> {
        if self.secret.len() > SHA256_BLOCK_LEN {
            let mut hasher = Sha256::new();
            hasher.update(&self.secret);
            hasher.finalize().to_vec()
        } else {
            self.secret.clone()
        }
    }

    /// Lowercase hex HMAC-SHA256 of `body` under the shared secret.
    fn sign(&self, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.effective_key())
            .expect("effective_key is never longer than the SHA-256 block size");
        mac.update(body);
        encode_hex(&mac.finalize().into_bytes())
    }

    /// Constant-time HMAC comparison (the `hmac` crate implements `verify_slice` with
    /// `subtle`, so the comparison does not leak how many bytes matched).
    fn verify_mac(&self, body: &[u8], expected: &[u8]) -> bool {
        let mut mac = HmacSha256::new_from_slice(&self.effective_key())
            .expect("effective_key is never longer than the SHA-256 block size");
        mac.update(body);
        mac.verify_slice(expected).is_ok()
    }
}

impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Auth")
            .field("secret", &"<redacted>")
            .finish()
    }
}

/// A successfully verified ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    /// Game session the ticket grants access to.
    pub session_id: Uuid,
    /// Player the ticket was issued to.
    pub player_id: Uuid,
    /// Expiration time in whole Unix seconds; the ticket is valid while
    /// `now < expires_at_unix`.
    pub expires_at_unix: i64,
}

impl Ticket {
    /// Whether the ticket is already expired at `now_unix` (whole Unix seconds).
    pub fn is_expired_at(&self, now_unix: i64) -> bool {
        now_unix >= self.expires_at_unix
    }

    /// Whether the ticket is already expired at the current wall-clock time.
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(unix_seconds(SystemTime::now()))
    }
}

/// Reasons a ticket can be rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    /// The string does not have the `v1.<session_id>.<player_id>.<unix_expiry>.<hex_hmac>`
    /// shape: wrong version, bad UUIDs, bad number, bad hex or wrong signature length.
    #[error("malformed ticket: expected `v1.<session_id>.<player_id>.<unix_expiry>.<hex_hmac>`")]
    Malformed,
    /// The HMAC does not match the shared secret (tampered payload or signature).
    #[error("ticket signature is invalid")]
    BadSignature,
    /// The ticket is well-formed and signed, but past its expiry.
    #[error("ticket has expired")]
    Expired,
    /// The ticket belongs to a different game session than expected.
    #[error("ticket belongs to a different session")]
    WrongSession,
}

// ============================================================================
// Internals
// ============================================================================

/// The `v1.<session_id>.<player_id>.<expiry>` fields, decoded and type-checked, before the
/// signature check.
struct ParsedTicket {
    session_id: Uuid,
    player_id: Uuid,
    expires_at_unix: i64,
    mac: Vec<u8>,
}

impl ParsedTicket {
    /// Checks the wire format of a `body`/`mac_hex` pair. Only checked operations are
    /// used, so arbitrary input can never panic here.
    fn parse(body: &str, mac_hex: &str) -> Result<Self, VerifyError> {
        if body.is_empty() || mac_hex.is_empty() {
            return Err(VerifyError::Malformed);
        }

        let mut fields = body.split('.');
        let version = fields.next().ok_or(VerifyError::Malformed)?;
        let session_id = fields.next().ok_or(VerifyError::Malformed)?;
        let player_id = fields.next().ok_or(VerifyError::Malformed)?;
        let expiry = fields.next().ok_or(VerifyError::Malformed)?;
        if fields.next().is_some() {
            return Err(VerifyError::Malformed);
        }

        if version != VERSION {
            return Err(VerifyError::Malformed);
        }
        let session_id = Uuid::parse_str(session_id).map_err(|_| VerifyError::Malformed)?;
        let player_id = Uuid::parse_str(player_id).map_err(|_| VerifyError::Malformed)?;
        let expires_at_unix: i64 = expiry.parse().map_err(|_| VerifyError::Malformed)?;

        let mac = decode_hex(mac_hex).ok_or(VerifyError::Malformed)?;
        if mac.len() != MAC_LEN {
            return Err(VerifyError::Malformed);
        }

        Ok(Self {
            session_id,
            player_id,
            expires_at_unix,
            mac,
        })
    }
}

/// Whole Unix seconds of `now`, saturating at `i64::MAX` (and `0` before the epoch).
fn unix_seconds(now: SystemTime) -> i64 {
    match now.duration_since(UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_secs().min(i64::MAX as u64) as i64,
        Err(_) => 0,
    }
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const SESSION_ID: &str = "5e2f9f3a-8b4d-4c1e-9a7b-2d0e6f8a1c3b";
    const PLAYER_ID: &str = "7d0f1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b";

    thread_local! {
        /// Wall clock used by [`fake_now`]; each test thread controls its own copy, so
        /// parallel tests can never race on the fake time.
        static FAKE_SECS: Cell<i64> = Cell::new(1_800_000_000);
    }

    fn fake_now() -> SystemTime {
        let secs = FAKE_SECS.with(|cell| cell.get().max(0)) as u64;
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn set_fake_time(secs: i64) {
        FAKE_SECS.with(|cell| cell.set(secs));
    }

    /// Builds an `Auth` with an injectable clock (the real `Auth::new` uses the wall clock).
    fn auth_with_clock(secret: &[u8], clock: fn() -> SystemTime) -> Auth {
        Auth {
            secret: secret.to_vec(),
            clock,
        }
    }

    fn auth(secret: impl Into<Vec<u8>>) -> Auth {
        Auth::new(secret)
    }

    fn session() -> Uuid {
        Uuid::parse_str(SESSION_ID).unwrap()
    }

    fn player() -> Uuid {
        Uuid::parse_str(PLAYER_ID).unwrap()
    }

    /// Replaces one `.`-separated field of a ticket with `replacement`.
    fn replace_field(ticket: &str, index: usize, replacement: &str) -> String {
        let parts: Vec<&str> = ticket.split('.').collect();
        assert!(index < parts.len(), "field index out of range");
        let mut out = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                out.push('.');
            }
            out.push_str(if i == index { replacement } else { part });
        }
        out
    }

    #[test]
    fn issued_ticket_verifies_and_roundtrips_fields() {
        let auth = auth(b"roundtrip-secret");
        let sid = Uuid::new_v4();
        let pid = Uuid::new_v4();

        let ticket = auth.issue(sid, pid, Duration::from_secs(3600));
        let parsed = auth.verify(&ticket, sid).expect("fresh ticket must verify");

        assert_eq!(parsed.session_id, sid);
        assert_eq!(parsed.player_id, pid);
        let now = unix_seconds(SystemTime::now());
        assert!(parsed.expires_at_unix > now + 3590);
        assert!(parsed.expires_at_unix <= now + 3601);
    }

    #[test]
    fn issue_is_deterministic_for_a_fixed_clock() {
        let auth = auth_with_clock(b"shared-secret", fake_now);
        let (sid, pid) = (session(), player());

        set_fake_time(1_800_000_000);
        let first = auth.issue(sid, pid, Duration::from_secs(45));
        let second = auth.issue(sid, pid, Duration::from_secs(45));
        assert_eq!(first, second);

        let prefix = format!("v1.{sid}.{pid}.1800000045.");
        let mac = first
            .strip_prefix(&prefix)
            .unwrap_or_else(|| panic!("ticket {first} must start with {prefix}"));
        assert_eq!(mac.len(), 64, "HMAC-SHA256 hex must be 64 chars: {first}");
        assert!(
            mac.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "MAC must be lowercase hex: {mac}"
        );
    }

    #[test]
    fn tickets_differ_for_different_players_and_sessions() {
        let auth = auth_with_clock(b"secret", fake_now);
        set_fake_time(1_800_000_000);

        let a = auth.issue(Uuid::nil(), Uuid::nil(), Duration::from_secs(60));
        let b = auth.issue(Uuid::nil(), Uuid::max(), Duration::from_secs(60));
        let c = auth.issue(Uuid::max(), Uuid::max(), Duration::from_secs(60));
        assert_ne!(a, b);
        assert_ne!(b, c);
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let auth = auth(b"secret");
        let (sid, pid) = (session(), player());
        let ticket = auth.issue(sid, pid, Duration::from_secs(3600));

        // Expiry digit changed: still well-formed, but no longer covered by the signature.
        let tampered = replace_field(&ticket, 3, "9999999999");
        assert_eq!(auth.verify(&tampered, sid), Err(VerifyError::BadSignature));

        // Session id changed to another valid UUID: same thing.
        let tampered = replace_field(&ticket, 1, PLAYER_ID);
        assert_eq!(auth.verify(&tampered, sid), Err(VerifyError::BadSignature));
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let auth = auth(b"secret");
        let sid = session();
        let ticket = auth.issue(sid, player(), Duration::from_secs(3600));

        let tampered = replace_field(&ticket, 4, &"0".repeat(64));
        assert_eq!(auth.verify(&tampered, sid), Err(VerifyError::BadSignature));
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let sid = session();
        let ticket = auth("secret-a").issue(sid, player(), Duration::from_secs(3600));
        assert_eq!(
            auth("secret-b").verify(&ticket, sid),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn wrong_session_is_rejected() {
        let auth = auth(b"secret");
        let ticket = auth.issue(session(), player(), Duration::from_secs(3600));
        assert_eq!(
            auth.verify(&ticket, Uuid::new_v4()),
            Err(VerifyError::WrongSession)
        );
    }

    #[test]
    fn expired_ticket_is_rejected() {
        let auth = auth_with_clock(b"secret", fake_now);
        let sid = session();

        set_fake_time(1_800_000_000);
        let ticket = auth.issue(sid, player(), Duration::from_secs(60));

        set_fake_time(1_800_000_000 + 59);
        assert!(
            auth.verify(&ticket, sid).is_ok(),
            "ticket must stay valid one second before expiry"
        );

        set_fake_time(1_800_000_000 + 60);
        assert_eq!(auth.verify(&ticket, sid), Err(VerifyError::Expired));
    }

    #[test]
    fn zero_ttl_ticket_is_immediately_expired() {
        let auth = auth_with_clock(b"secret", fake_now);
        let sid = session();
        set_fake_time(1_800_000_000);

        let ticket = auth.issue(sid, player(), Duration::ZERO);
        assert_eq!(auth.verify(&ticket, sid), Err(VerifyError::Expired));
    }

    #[test]
    fn negative_expiry_is_expired() {
        let auth = auth(b"secret");
        let sid = session();

        // Craft a *validly signed* ticket whose expiry lies in the past.
        let body = format!("v1.{sid}.{}.-1", player());
        let mac = auth.sign(body.as_bytes());
        let ticket = format!("{body}.{mac}");

        assert_eq!(auth.verify(&ticket, sid), Err(VerifyError::Expired));
    }

    #[test]
    fn malformed_tickets_are_rejected_without_panicking() {
        let auth = auth(b"secret");
        let sid = session();
        let zero_mac = "0".repeat(64);
        let short_mac = "0".repeat(62);
        let bad_hex_mac = "g".repeat(64);

        let cases = [
            String::new(),
            "v1".to_string(),
            "v1.".to_string(),
            ".".to_string(),
            "v1....".to_string(),
            "v1.abc".to_string(),
            "v1.not-a-uuid.not-a-uuid.123.zz".to_string(),
            format!("v2.{SESSION_ID}.{PLAYER_ID}.123.{zero_mac}"),
            format!("v1.{SESSION_ID}.{PLAYER_ID}.not-a-number.{zero_mac}"),
            format!("v1.{SESSION_ID}.{PLAYER_ID}.123.{short_mac}"),
            format!("v1.{SESSION_ID}.{PLAYER_ID}.123.{bad_hex_mac}"),
            format!("v1.{SESSION_ID}.{PLAYER_ID}.123.{zero_mac}."),
            format!("v1.{SESSION_ID}.{PLAYER_ID}.123.extra.{zero_mac}"),
            format!("v1.{SESSION_ID}.{PLAYER_ID}..{zero_mac}"),
            format!("v1..{PLAYER_ID}.123.{zero_mac}"),
        ];

        for case in &cases {
            assert_eq!(
                auth.verify(case, sid),
                Err(VerifyError::Malformed),
                "case: {case:?}"
            );
        }
    }

    #[test]
    fn uppercase_hex_signature_is_accepted() {
        let auth = auth(b"secret");
        let sid = session();
        let ticket = auth.issue(sid, player(), Duration::from_secs(3600));

        let (body, mac_hex) = ticket.rsplit_once('.').unwrap();
        let upper = format!("{body}.{}", mac_hex.to_ascii_uppercase());
        assert!(auth.verify(&upper, sid).is_ok());
    }

    #[test]
    fn long_and_empty_secrets_work() {
        let sid = session();
        for len in [0usize, 1, 64, 65, 128, 1024] {
            let secret: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let auth = auth_with_clock(&secret, fake_now);
            set_fake_time(1_800_000_000);
            let ticket = auth.issue(sid, player(), Duration::from_secs(300));
            assert!(
                auth.verify(&ticket, sid).is_ok(),
                "secret of length {len} must round-trip"
            );
        }
    }

    #[test]
    fn debug_does_not_leak_the_secret() {
        let rendered = format!("{:?}", auth("super-secret-value"));
        assert!(!rendered.contains("super-secret-value"));
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn ticket_expiry_helpers_match_verify_semantics() {
        let ticket = Ticket {
            session_id: Uuid::nil(),
            player_id: Uuid::nil(),
            expires_at_unix: 100,
        };
        assert!(!ticket.is_expired_at(99));
        assert!(ticket.is_expired_at(100));
        assert!(ticket.is_expired_at(101));

        let far_future = Ticket {
            session_id: Uuid::nil(),
            player_id: Uuid::nil(),
            expires_at_unix: i64::MAX,
        };
        assert!(!far_future.is_expired());
    }

    #[test]
    fn verify_errors_display_and_are_errors() {
        fn assert_error<T: std::error::Error>(_: &T) {}

        for error in [
            VerifyError::Malformed,
            VerifyError::BadSignature,
            VerifyError::Expired,
            VerifyError::WrongSession,
        ] {
            assert!(!error.to_string().is_empty());
            assert_error(&error);
        }
    }

    #[test]
    fn verify_never_panics_on_garbage() {
        // Deterministic pseudo-random fuzz (no `rand` dependency): the ticket alphabet
        // plus delimiters and junk, so malformed shapes and almost-valid shapes both occur.
        let auth = auth(b"fuzz-secret");
        let sid = Uuid::nil();
        let charset = b"v1.x-abcdef0123456789ABCDEF:Z!@#";
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;

        for _ in 0..20_000 {
            let len = ((state >> 32) % 48) as usize;
            let mut candidate = String::with_capacity(len);
            for _ in 0..len {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                candidate.push(charset[((state >> 33) as usize) % charset.len()] as char);
            }
            let _ = auth.verify(&candidate, sid); // must never panic
        }
    }
}
