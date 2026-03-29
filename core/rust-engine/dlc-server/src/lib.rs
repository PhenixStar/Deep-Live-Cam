//! Public library surface for dlc-server.
//!
//! Exposes `build_router` and `test_state` so integration tests can drive
//! the Axum router in-process without binding a real TCP socket.

pub mod state;
pub mod router;
pub mod profiles;

/// Convenience re-export for tests: a `ServerState` with no models loaded.
pub use router::test_state;
