//! Minimal safe wrapper around the vendored ENet 1.3.18 C library.
//!
//! # Why `unsafe` is confined here
//!
//! ENet is a C library: every call takes raw pointers with lifetime and
//! aliasing contracts that the compiler cannot check. This module is the only
//! place in `max_net` that touches them. The invariants we uphold are:
//!
//! * a [`Host`] owns exactly one `ENetHost*` and destroys it in [`Drop`];
//! * a [`Peer`] is only ever passed back to the [`Host`] that produced it;
//! * [`Host`] is `Send` (the session moves it between async task threads) but
//!   **not** `Sync` — all mutation goes through `&mut self`, so no two threads
//!   can call into the same host concurrently;
//! * received packets are copied into a `Vec<u8>` and immediately destroyed,
//!   because ENet transfers packet ownership to the application;
//! * sent packets are destroyed by us if `enet_peer_send` rejects them, since
//!   ownership only transfers on success.
//!
//! The version is pinned to the same 1.3.18 sources the C++ port builds
//! (`dependencies/enet-1.3.18.tar.gz`, `ENET_VERSION = 0x010312`); the
//! handshake compares this value byte-for-byte (`max-brh-docs/protocol.md` §7).

// This is the only module of the crate allowed to use `unsafe`: the crate root
// denies it (`lib.rs`) and re-enables it here only, where the invariants above
// are upheld.
#![allow(unsafe_code)]

use std::ffi::{c_char, c_int, c_void, CString};
use std::net::Ipv4Addr;
use std::ptr::NonNull;
use std::sync::OnceLock;

/// ENet version compiled into the vendored library (`ENET_VERSION`).
pub const ENET_VERSION: u32 = 0x010312;
/// `ENET_PACKET_FLAG_RELIABLE` — every game packet uses reliable delivery.
pub const ENET_PACKET_FLAG_RELIABLE: u32 = 1 << 0;
/// `ENET_HOST_ANY` bind address.
pub const ENET_HOST_ANY: u32 = 0;
/// Channels used by the protocol: 0 = TP handshake, 1 = APPL game data.
pub const CHANNEL_COUNT: usize = 2;
/// Peer slots per host (`enet_host_create(..., 32, 2, 0, 0)`).
pub const PEER_COUNT: usize = 32;
/// Channel 0 — transport handshake.
pub const CHANNEL_TP: u8 = 0;
/// Channel 1 — application/game protocol.
pub const CHANNEL_APPL: u8 = 1;
/// ENet `enet_host_service` timeout used by the session loop (10 ms).
pub const SERVICE_TIMEOUT_MS: u32 = 10;

const ENET_EVENT_TYPE_NONE: c_int = 0;
const ENET_EVENT_TYPE_CONNECT: c_int = 1;
const ENET_EVENT_TYPE_DISCONNECT: c_int = 2;
const ENET_EVENT_TYPE_RECEIVE: c_int = 3;

/// `ENetAddress` (`include/enet/enet.h`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ENetAddress {
    /// IPv4 address in network byte order.
    pub host: u32,
    /// UDP port in host byte order.
    pub port: u16,
}

/// Opaque `ENetHost`.
#[repr(C)]
pub struct ENetHost {
    _private: [u8; 0],
}

/// Opaque `ENetPeer`.
#[repr(C)]
pub struct ENetPeer {
    _private: [u8; 0],
}

/// `ENetPacket` — only the fields the wrapper reads are modelled, but the
/// layout must stay exact because `enet_packet_create` allocates it.
#[repr(C)]
#[allow(dead_code)]
struct ENetPacket {
    reference_count: usize,
    flags: u32,
    data: *mut u8,
    data_length: usize,
    free_callback: Option<unsafe extern "C" fn(*mut ENetPacket)>,
    user_data: *mut c_void,
}

/// `ENetEvent` (`include/enet/enet.h`).
#[repr(C)]
struct ENetEvent {
    event_type: c_int,
    peer: *mut ENetPeer,
    channel_id: u8,
    data: u32,
    packet: *mut ENetPacket,
}

unsafe extern "C" {
    fn enet_initialize() -> c_int;
    fn enet_linked_version() -> u32;
    fn enet_address_set_host_ip(address: *mut ENetAddress, host_name: *const c_char) -> c_int;
    fn enet_host_create(
        address: *const ENetAddress,
        peer_count: usize,
        channel_limit: usize,
        incoming_bandwidth: u32,
        outgoing_bandwidth: u32,
    ) -> *mut ENetHost;
    fn enet_host_destroy(host: *mut ENetHost);
    fn enet_host_connect(
        host: *mut ENetHost,
        address: *const ENetAddress,
        channel_count: usize,
        data: u32,
    ) -> *mut ENetPeer;
    fn enet_host_service(host: *mut ENetHost, event: *mut ENetEvent, timeout: u32) -> c_int;
    fn enet_host_flush(host: *mut ENetHost);
    fn enet_peer_send(peer: *mut ENetPeer, channel_id: u8, packet: *mut ENetPacket) -> c_int;
    fn enet_peer_disconnect(peer: *mut ENetPeer, data: u32);
    fn enet_peer_disconnect_later(peer: *mut ENetPeer, data: u32);
    fn enet_peer_disconnect_now(peer: *mut ENetPeer, data: u32);
    fn enet_packet_create(data: *const c_void, length: usize, flags: u32) -> *mut ENetPacket;
    fn enet_packet_destroy(packet: *mut ENetPacket);
}

/// Errors returned by the ENet wrapper.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnetError {
    /// `enet_initialize()` failed.
    #[error("enet_initialize failed with code {0}")]
    Initialize(c_int),
    /// The linked library is not the ENet 1.3.18 the protocol requires.
    #[error("linked ENet version 0x{found:06X} does not match required 0x{expected:06X}")]
    VersionMismatch {
        /// Linked version.
        found: u32,
        /// Required version.
        expected: u32,
    },
    /// `enet_host_create()` returned null.
    #[error("enet_host_create returned null")]
    HostCreate,
    /// `enet_host_connect()` returned null.
    #[error("enet_host_connect returned null")]
    Connect,
    /// The IP literal could not be parsed by ENet.
    #[error("invalid IPv4 address {0}")]
    Address(String),
    /// `enet_peer_send()` failed.
    #[error("enet_peer_send failed with code {0}")]
    Send(c_int),
    /// `enet_host_service()` failed.
    #[error("enet_host_service failed with code {0}")]
    Service(c_int),
}

/// Initializes ENet once per process and returns the linked version.
///
/// ENet's global initialization is reference-count-free in practice; the
/// protocol only ever needs it once per process, so `enet_deinitialize` is
/// deliberately not called (the C++ port also keeps it initialized).
pub fn initialize() -> Result<u32, EnetError> {
    static INIT: OnceLock<Result<u32, EnetError>> = OnceLock::new();
    let result = INIT
        .get_or_init(|| {
            // SAFETY: `enet_initialize` has no preconditions; it is called at
            // most once thanks to `OnceLock`.
            let code = unsafe { enet_initialize() };
            if code != 0 {
                return Err(EnetError::Initialize(code));
            }
            // SAFETY: pure accessor, valid after initialization.
            let version = unsafe { enet_linked_version() };
            if version != ENET_VERSION {
                return Err(EnetError::VersionMismatch {
                    found: version,
                    expected: ENET_VERSION,
                });
            }
            Ok(version)
        })
        .clone();
    result
}

/// Version of the linked ENet library (`0x010312`).
pub fn linked_version() -> Result<u32, EnetError> {
    initialize()
}

/// Parses an IPv4 literal into an [`ENetAddress`].
pub fn address(ip: Ipv4Addr, port: u16) -> Result<ENetAddress, EnetError> {
    initialize()?;
    let mut addr = ENetAddress {
        host: ENET_HOST_ANY,
        port,
    };
    let text = ip.to_string();
    let c_text = CString::new(text.clone()).map_err(|_| EnetError::Address(text.clone()))?;
    // SAFETY: `c_text` is a valid NUL-terminated string; `addr` is a valid
    // writable `ENetAddress` for the duration of the call.
    let code = unsafe { enet_address_set_host_ip(&mut addr, c_text.as_ptr()) };
    if code != 0 {
        return Err(EnetError::Address(text));
    }
    Ok(addr)
}

/// A peer handle owned by a [`Host`].
#[derive(Debug, Clone, Copy)]
pub struct Peer {
    ptr: NonNull<ENetPeer>,
}

// SAFETY: a `Peer` is a non-owning handle that is only dereferenced by the
// `Host` that produced it. The session moves the pair between tokio worker
// threads as a unit; no thread shares the handle concurrently (`Peer` is not
// `Copy` across threads by itself, and the host methods take `&mut self`).
unsafe impl Send for Peer {}

impl Peer {
    /// Borrows a raw peer pointer, rejecting null.
    fn from_raw(ptr: *mut ENetPeer) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Raw pointer for ENet calls.
    fn as_ptr(self) -> *mut ENetPeer {
        self.ptr.as_ptr()
    }

    /// Identity of this peer slot inside its [`Host`].
    ///
    /// The value is unique while the slot is occupied and is used by the
    /// session/host bookkeeping as a map key; ENet may reuse the slot after a
    /// disconnect, and callers must drop the entry when they observe
    /// [`EventKind::Disconnect`].
    pub fn key(self) -> usize {
        self.ptr.as_ptr() as usize
    }
}

/// Event kind delivered by [`Host::service`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// Nothing happened within the timeout.
    None,
    /// The peer completed the ENet handshake.
    Connect,
    /// The peer disconnected (or timed out).
    Disconnect,
    /// A reliable packet arrived.
    Receive,
}

/// One ENet event.
#[derive(Debug, Clone)]
pub struct Event {
    /// Event kind.
    pub kind: EventKind,
    /// Peer that generated the event (if any).
    pub peer: Option<Peer>,
    /// Channel of a received packet.
    pub channel: u8,
    /// Event data (ENet-specific, 0 in this protocol).
    pub data: u32,
    /// Payload of a received packet, copied out of ENet ownership.
    pub payload: Option<Vec<u8>>,
}

/// One ENet host (client or server).
///
/// `Send` is implemented because the owning session moves the value between
/// tokio worker threads; `Sync` is deliberately **not** implemented, and all
/// methods mutate through `&mut self`, so concurrent use is impossible in safe
/// code.
pub struct Host {
    ptr: NonNull<ENetHost>,
}

// SAFETY: `Host` is an owning handle to a single ENet host. Moving it between
// threads is sound because no other thread has access; `Sync` is not
// implemented, so shared references cannot be used concurrently.
unsafe impl Send for Host {}

impl std::fmt::Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Host").field("ptr", &self.ptr).finish()
    }
}

impl Host {
    /// Creates a client host (`enet_host_create(nullptr, peer_count, channels,
    /// 0, 0)`).
    pub fn client(channels: usize) -> Result<Self, EnetError> {
        initialize()?;
        // SAFETY: a null address means "any local port", as in the C++ client.
        let ptr = unsafe { enet_host_create(std::ptr::null(), PEER_COUNT, channels, 0, 0) };
        NonNull::new(ptr)
            .map(|ptr| Self { ptr })
            .ok_or(EnetError::HostCreate)
    }

    /// Creates a server host bound to `ip:port`.
    pub fn server(ip: Ipv4Addr, port: u16, channels: usize) -> Result<Self, EnetError> {
        initialize()?;
        let address = address(ip, port)?;
        // SAFETY: `address` is valid for the duration of the call; ENet copies
        // it into the host.
        let ptr = unsafe { enet_host_create(&address, PEER_COUNT, channels, 0, 0) };
        NonNull::new(ptr)
            .map(|ptr| Self { ptr })
            .ok_or(EnetError::HostCreate)
    }

    /// Starts a connection; the [`EventKind::Connect`] event follows later.
    pub fn connect(
        &mut self,
        address: ENetAddress,
        channel_count: usize,
    ) -> Result<Peer, EnetError> {
        // SAFETY: `self.ptr` is a live host and `address` is a valid
        // ENetAddress; ENet copies it.
        let ptr = unsafe { enet_host_connect(self.ptr.as_ptr(), &address, channel_count, 0) };
        Peer::from_raw(ptr).ok_or(EnetError::Connect)
    }

    /// Services the host for up to `timeout_ms`; `None` means no event.
    pub fn service(&mut self, timeout_ms: u32) -> Result<Option<Event>, EnetError> {
        let mut raw = ENetEvent {
            event_type: ENET_EVENT_TYPE_NONE,
            peer: std::ptr::null_mut(),
            channel_id: 0,
            data: 0,
            packet: std::ptr::null_mut(),
        };
        // SAFETY: `self.ptr` is live and `raw` is a valid writable event.
        let code = unsafe { enet_host_service(self.ptr.as_ptr(), &mut raw, timeout_ms) };
        if code < 0 {
            return Err(EnetError::Service(code));
        }
        if code == 0 {
            return Ok(None);
        }

        let mut event = Event {
            kind: match raw.event_type {
                ENET_EVENT_TYPE_CONNECT => EventKind::Connect,
                ENET_EVENT_TYPE_DISCONNECT => EventKind::Disconnect,
                ENET_EVENT_TYPE_RECEIVE => EventKind::Receive,
                _ => EventKind::None,
            },
            peer: Peer::from_raw(raw.peer),
            channel: raw.channel_id,
            data: raw.data,
            payload: None,
        };

        if raw.event_type == ENET_EVENT_TYPE_RECEIVE && !raw.packet.is_null() {
            // SAFETY: for a RECEIVE event ENet hands packet ownership to the
            // application; the fields are valid until `enet_packet_destroy`.
            let packet = unsafe { &*raw.packet };
            let payload = if packet.data.is_null() || packet.data_length == 0 {
                Vec::new()
            } else {
                // SAFETY: ENet guarantees `data` points to `data_length` bytes.
                unsafe { std::slice::from_raw_parts(packet.data, packet.data_length) }.to_vec()
            };
            // SAFETY: ownership was transferred to us by ENet; each received
            // packet is destroyed exactly once here.
            unsafe { enet_packet_destroy(raw.packet) };
            event.payload = Some(payload);
        }

        Ok(Some(event))
    }

    /// Sends `payload` reliably on `channel`.
    pub fn send(&mut self, peer: Peer, channel: u8, payload: &[u8]) -> Result<(), EnetError> {
        // SAFETY: `payload` is a valid slice for the duration of the call;
        // `enet_packet_create` copies it (no NO_ALLOCATE flag).
        let packet = unsafe {
            enet_packet_create(
                payload.as_ptr().cast::<c_void>(),
                payload.len(),
                ENET_PACKET_FLAG_RELIABLE,
            )
        };
        let packet = NonNull::new(packet).ok_or(EnetError::Send(-1))?;

        // SAFETY: both pointers are live and belong to this host.
        let code = unsafe { enet_peer_send(peer.as_ptr(), channel, packet.as_ptr()) };
        if code != 0 {
            // SAFETY: on failure ENet does not take ownership, so we destroy
            // the packet ourselves to avoid a leak.
            unsafe { enet_packet_destroy(packet.as_ptr()) };
            return Err(EnetError::Send(code));
        }
        Ok(())
    }

    /// Flushes queued packets to the socket (called after sends for latency).
    pub fn flush(&mut self) {
        // SAFETY: `self.ptr` is live.
        unsafe { enet_host_flush(self.ptr.as_ptr()) };
    }

    /// Sends a disconnect request and waits for the peer to acknowledge.
    pub fn disconnect(&mut self, peer: Peer, data: u32) {
        // SAFETY: peer belongs to this host.
        unsafe { enet_peer_disconnect(peer.as_ptr(), data) };
    }

    /// Queues a disconnect after the outgoing queue drains.
    pub fn disconnect_later(&mut self, peer: Peer, data: u32) {
        // SAFETY: peer belongs to this host.
        unsafe { enet_peer_disconnect_later(peer.as_ptr(), data) };
    }

    /// Drops the connection immediately (used after a TP 03 rejection).
    pub fn disconnect_now(&mut self, peer: Peer, data: u32) {
        // SAFETY: peer belongs to this host.
        unsafe { enet_peer_disconnect_now(peer.as_ptr(), data) };
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: `self.ptr` was created by `enet_host_create` and is owned
        // exclusively by this value.
        unsafe { enet_host_destroy(self.ptr.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn service_with_zero_timeout_returns_no_event() {
        let mut host = Host::client(CHANNEL_COUNT).unwrap();
        assert!(host.service(0).unwrap().is_none());
    }
}
