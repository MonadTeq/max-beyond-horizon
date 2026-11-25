use super::*;

#[test]
fn scalars_round_trip_little_endian() {
    let mut w = Writer::new();
    w.write_u8(0xAB);
    w.write_i8(-2);
    w.write_u16(0x1234);
    w.write_i16(-300);
    w.write_u32(0xDEAD_BEEF);
    w.write_i32(-123_456);
    w.write_bool(true);
    w.write_bool(false);
    assert_eq!(
        w.as_slice(),
        &[
            0xAB, 0xFE, 0x34, 0x12, 0xD4, 0xFE, 0xEF, 0xBE, 0xAD, 0xDE, 0xC0, 0x1D, 0xFE, 0xFF,
            0x01, 0x00
        ]
    );

    let bytes = w.finish();
    let mut r = Reader::new(&bytes);
    assert_eq!(r.read_u8().unwrap(), 0xAB);
    assert_eq!(r.read_i8().unwrap(), -2);
    assert_eq!(r.read_u16().unwrap(), 0x1234);
    assert_eq!(r.read_i16().unwrap(), -300);
    assert_eq!(r.read_u32().unwrap(), 0xDEAD_BEEF);
    assert_eq!(r.read_i32().unwrap(), -123_456);
    assert!(r.read_bool().unwrap());
    assert!(!r.read_bool().unwrap());
    r.finish().unwrap();
}

#[test]
fn std_string_is_u32_length_without_nul() {
    let mut w = Writer::new();
    w.write_string("abc").unwrap();
    assert_eq!(w.as_slice(), &[3, 0, 0, 0, b'a', b'b', b'c']);

    let bytes = w.finish();
    let mut r = Reader::new(&bytes);
    assert_eq!(r.read_string().unwrap(), "abc");
    r.finish().unwrap();
}

#[test]
fn smart_string_is_u16_length_including_nul() {
    let mut w = Writer::new();
    w.write_smart_string("abc").unwrap();
    assert_eq!(w.as_slice(), &[4, 0, b'a', b'b', b'c', 0]);

    let bytes = w.finish();
    let mut r = Reader::new(&bytes);
    assert_eq!(r.read_smart_string().unwrap(), "abc");
    r.finish().unwrap();
}

#[test]
fn smart_string_accepts_missing_terminator_leniently() {
    let bytes = [3, 0, b'a', b'b', b'c'];
    let mut r = Reader::new(&bytes);
    assert_eq!(r.read_smart_string().unwrap(), "abc");
}

#[test]
fn strings_are_bounded() {
    let too_long = u32::MAX.to_le_bytes();
    let mut r = Reader::new(&too_long);
    assert!(matches!(
        r.read_string(),
        Err(WireError::StringTooLong { .. })
    ));
    let smart = [0xFF, 0xFF];
    assert!(matches!(
        Reader::new(&smart).read_smart_string(),
        Err(WireError::StringTooLong { .. })
    ));
}

#[test]
fn reads_past_end_is_an_error() {
    let mut r = Reader::new(&[1, 2]);
    assert_eq!(r.read_u16().unwrap(), 0x0201);
    assert!(matches!(
        r.read_u8(),
        Err(WireError::UnexpectedEof { offset: 2, need: 1 })
    ));
}

#[test]
fn names_are_nul_terminated_and_truncated_to_29() {
    let name = WireName::from("abcdefghijklmnopqrstuvwxyz0123456789");
    assert_eq!(name.as_str(), "abcdefghijklmnopqrstuvwxyz012");
    assert_eq!(name.as_bytes()[29], 0);
    assert_eq!(WireName::EMPTY.as_str(), "");
}

#[test]
fn trailing_bytes_are_detected() {
    let r = Reader::new(&[0, 1]);
    assert!(matches!(r.finish(), Err(WireError::TrailingBytes(2))));
}
