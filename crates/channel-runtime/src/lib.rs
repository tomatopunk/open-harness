//! Pluggable IM channel drivers.

pub mod driver;
pub mod registry;

pub use driver::{ChannelDriver, ChannelEnvelope, ChannelError, NormalizedCommand};
pub use registry::ChannelRegistry;
