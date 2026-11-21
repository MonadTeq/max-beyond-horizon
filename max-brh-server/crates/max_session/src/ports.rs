//! UDP port pool for game sessions.
//!
//! Every session runs its own headless M.A.X. Port process on a dedicated UDP port. The
//! allocator hands out free ports from a configured range and takes them back when a session
//! stops.

use std::collections::HashSet;
use std::sync::Mutex;

/// Allocates and releases UDP ports from a fixed range.
#[derive(Debug)]
pub struct PortAllocator {
    start: u16,
    end: u16,
    used: Mutex<HashSet<u16>>,
}

impl PortAllocator {
    /// Creates an allocator for the inclusive range `start..=end`.
    pub fn new(range: std::ops::RangeInclusive<u16>) -> Self {
        Self {
            start: *range.start(),
            end: *range.end(),
            used: Mutex::new(HashSet::new()),
        }
    }

    /// Allocates a free port or returns `None` when the range is exhausted.
    pub fn allocate(&self) -> Option<u16> {
        let mut used = self.used.lock().expect("port allocator poisoned");

        for port in self.start..=self.end {
            if !used.contains(&port) {
                used.insert(port);
                return Some(port);
            }
        }

        None
    }

    /// Returns a previously allocated port to the pool.
    pub fn release(&self, port: u16) {
        self.used.lock().expect("port allocator poisoned").remove(&port);
    }

    /// Number of ports currently allocated.
    pub fn in_use(&self) -> usize {
        self.used.lock().expect("port allocator poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_without_reuse() {
        let allocator = PortAllocator::new(20000..=20002);
        let a = allocator.allocate().unwrap();
        let b = allocator.allocate().unwrap();
        let c = allocator.allocate().unwrap();

        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        assert_eq!(allocator.in_use(), 3);
    }

    #[test]
    fn release_makes_the_port_available_again() {
        let allocator = PortAllocator::new(20000..=20000);
        let a = allocator.allocate().unwrap();

        assert!(allocator.allocate().is_none());

        allocator.release(a);

        assert_eq!(allocator.allocate(), Some(a));
    }

    #[test]
    fn exhaustion_reports_none() {
        let allocator = PortAllocator::new(20000..=20000);
        assert!(allocator.allocate().is_some());
        assert!(allocator.allocate().is_none());
        assert_eq!(allocator.in_use(), 1);
    }

    #[test]
    fn empty_range_never_allocates() {
        let allocator = PortAllocator::new(20010..=20000);
        assert!(allocator.allocate().is_none());
    }
}
