#![cfg_attr(not(feature = "dynamic"), no_std)] // TODO: Try to keep no_std even with dynamic.

pub mod utilities;

extern crate alloc;

mod definitions;
pub use definitions::*;

// messages::message_structs!("definitions");
