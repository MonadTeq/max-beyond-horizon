//! Prints FNV-1a hashes of the first N MVE frames (default — `maxint.mve`).
//!
//! Used to cross-check the C harness over the oracle's `nfPkDecomp`:
//!
//! ```sh
//! cargo run -p max_assets --example mve_hashes -- ../max-original/MAXINT.MVE 300
//! ```

use max_assets::mve::Mve;

fn fnv1a(data: &[u8]) -> u32 {
    let mut hash = 2_166_136_261u32;
    for &byte in data {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../max-original/MAXINT.MVE"
        )
        .to_string()
    });
    let count: usize = std::env::args()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(300);
    let mve = Mve::open(path).expect("MVE");
    let mut decoder = mve.decoder();
    for index in 0..count {
        let frame = decoder.next_frame().expect("decode").expect("frame");
        println!("frame {index} hash={:08X}", fnv1a(frame));
    }
}
