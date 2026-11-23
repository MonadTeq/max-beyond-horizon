//! Stage-A/B facade scenarios that compare the facade path with the legacy
//! `GameState` path (`enqueue` + `tick`, immediate `apply_command`).
//!
//! Stage B §7 removes the facade's `state()`/`state_mut()` accessors and makes
//! the `GameState` mutators `pub(crate)`, so the legacy reference path is no
//! longer expressible from an integration test. The scenarios therefore live
//! inside the crate (they are inline `#[cfg(test)]` modules for §7 purposes) and
//! reach the private `GameUniverse::state` field directly; the facade half keeps
//! using the public API unchanged.

mod gate3;
mod load_resume;
mod turn;
mod universe;
