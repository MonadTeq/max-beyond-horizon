use super::*;

#[test]
fn assets_smoke() {
    assert!(parse_res(b"").is_err());
    assert!(WrlMap::parse(b"").is_err());
    assert!(decode_sprite(b"").is_err());
}
