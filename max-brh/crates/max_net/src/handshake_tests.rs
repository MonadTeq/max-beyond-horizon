use super::*;

fn data_hash() -> DataHash {
    DataHash::from_bytes(b"abc", b"")
}

#[test]
fn sha256_vectors_are_the_known_ones() {
    let hash = data_hash();
    assert_eq!(
        data_hash().to_hex(),
        concat!(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        )
    );
    assert_eq!(DataHash::from_hex(&hash.to_hex()).unwrap(), hash);
}

#[test]
fn data_hash_parses_and_rejects_bad_hex() {
    assert!(DataHash::from_hex("00").is_err());
    assert!(DataHash::from_hex(&"z".repeat(128)).is_err());
}

#[test]
fn data_hash_from_files_and_dir() {
    let dir = std::env::temp_dir().join(format!(
            "max_net_data_hash_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MAX_RES_FILE), b"abc").unwrap();
    std::fs::write(dir.join(PATCHES_RES_FILE), b"").unwrap();
    assert_eq!(DataHash::from_data_dir(&dir).unwrap(), data_hash());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn tp00_layout_is_exact() {
    let handshake = Handshake {
        game_version: DEFAULT_GAME_VERSION,
        enet_version: ENET_VERSION,
        protocol_version: PROTOCOL_VERSION,
        ticket: "abc123".into(),
        data_hash: "deadbeef".into(),
    };
    let bytes = handshake.encode().unwrap();
    assert_eq!(
        bytes,
        [
            0x00, // type
            0x00, 0x08, 0x00, 0x00, // game_version 0.8.0
            0x12, 0x03, 0x01, 0x00, // enet_version 1.3.18
            0x04, 0x00, // protocol_version
            0x06, 0x00, 0x00, 0x00, b'a', b'b', b'c', b'1', b'2', b'3', // ticket
            0x08, 0x00, 0x00, 0x00, b'd', b'e', b'a', b'd', b'b', b'e', b'e',
            b'f', // hash
        ]
    );
    assert_eq!(Handshake::decode(&bytes).unwrap(), handshake);
}

#[test]
fn tp03_layout_and_reason_mapping() {
    let reject = Reject {
        game_version: DEFAULT_GAME_VERSION,
        enet_version: ENET_VERSION,
        protocol_version: PROTOCOL_VERSION,
        reason: RejectReason::SessionTicket,
        reason_text: "session ticket".into(),
    };
    let bytes = reject.encode().unwrap();
    assert_eq!(
        bytes[..12],
        [0x03, 0x00, 0x08, 0x00, 0x00, 0x12, 0x03, 0x01, 0x00, 0x04, 0x00, 0x02]
    );
    assert_eq!(&bytes[12..16], &[14, 0, 0, 0]);
    assert_eq!(&bytes[16..], b"session ticket");
    assert_eq!(Reject::decode(&bytes).unwrap(), reject);
    assert_eq!(RejectReason::from_u8(1), RejectReason::Version);
    assert_eq!(RejectReason::from_u8(2), RejectReason::SessionTicket);
    assert_eq!(RejectReason::from_u8(3), RejectReason::GameData);
    assert_eq!(RejectReason::from_u8(9), RejectReason::Unknown(9));
}

#[test]
fn transport_packet_dispatches_by_type() {
    let handshake = Handshake::new(DEFAULT_GAME_VERSION, "", data_hash());
    let hello = TransportPacket::Hello(handshake.clone());
    assert_eq!(
        TransportPacket::decode(&hello.encode().unwrap()).unwrap(),
        hello
    );
    let reject = Reject::new(&handshake, RejectReason::GameData);
    let packet = TransportPacket::Reject(reject);
    assert_eq!(
        TransportPacket::decode(&packet.encode().unwrap()).unwrap(),
        packet
    );
}

#[test]
fn validation_maps_mismatches_to_reasons() {
    let local = Handshake::new(DEFAULT_GAME_VERSION, "t", data_hash());

    let mut peer = local.clone();
    assert_eq!(local.validate_peer(&peer), Ok(()));

    peer.game_version = 1;
    assert_eq!(local.validate_peer(&peer), Err(RejectReason::Version));

    let mut peer = local.clone();
    peer.enet_version = 0x010311;
    assert_eq!(local.validate_peer(&peer), Err(RejectReason::Version));

    let mut peer = local.clone();
    peer.protocol_version = 5;
    assert_eq!(local.validate_peer(&peer), Err(RejectReason::Version));

    let mut peer = local.clone();
    peer.data_hash = "00".repeat(64);
    assert_eq!(local.validate_peer(&peer), Err(RejectReason::GameData));

    // The ticket is server-side only.
    let mut peer = local.clone();
    peer.ticket = "different".into();
    assert_eq!(local.validate_peer(&peer), Ok(()));
    assert_eq!(validate_ticket("t", "t"), Ok(()));
    assert_eq!(
        validate_ticket("t", "other"),
        Err(RejectReason::SessionTicket)
    );
    assert_eq!(validate_ticket("", "anything"), Ok(()));
}

#[test]
fn malformed_tp00_is_rejected() {
    let bytes = [0x00, 0x01];
    assert!(matches!(
        Handshake::decode(&bytes),
        Err(HandshakeError::Wire(WireError::UnexpectedEof { .. }))
    ));
    assert!(matches!(
        Handshake::decode(&[0x03, 0, 0, 0, 0]),
        Err(HandshakeError::UnexpectedType { .. })
    ));
}
