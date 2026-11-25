use super::*;

#[test]
fn linked_version_is_enet_1_3_18() {
    assert_eq!(initialize().unwrap(), 0x010312);
    assert_eq!(ENET_VERSION, 0x010312);
}

#[test]
fn client_and_server_hosts_can_be_created() {
    let client = Host::client(CHANNEL_COUNT).unwrap();
    assert!(format!("{client:?}").contains("Host"));
    let server = Host::server(Ipv4Addr::LOCALHOST, 0, CHANNEL_COUNT).unwrap();
    drop(server);
}

#[test]
fn addresses_are_network_byte_order() {
    let address = address(Ipv4Addr::new(127, 0, 0, 1), 31554).unwrap();
    assert_eq!(address.port, 31554);
    // The field is a raw `struct in_addr` in network byte order.
    assert_eq!(address.host.to_ne_bytes(), [127, 0, 0, 1]);
}

/// A client host must be serviceable before it sends anything.
///
/// Windows regression: with the C++ `NULL` address ENet left the UDP
/// socket unbound until the first send, and `WSARecvFrom` on an unbound
/// socket fails with `WSAEINVAL` (10022), which ENet surfaces as
/// `enet_host_service(...) == -1` — `Service(-1)` from this wrapper.
/// [`Host::client`] now binds an ephemeral port eagerly, so the very first
/// poll reports "no event" on every platform.
#[test]
fn service_with_zero_timeout_returns_no_event() {
    let mut host = Host::client(CHANNEL_COUNT).unwrap();
    assert!(host.service(0).unwrap().is_none());
}
