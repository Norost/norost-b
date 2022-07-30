//! # Common driver utilities
//!
//! This crate has a collection of types that are commonly in drivers.

#![cfg_attr(not(feature = "std"), no_std)]
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_slice)]
#![feature(new_uninit)]
#![cfg_attr(feature = "std", feature(norostb))]
#![cfg_attr(feature = "std", feature(read_buf))]

extern crate alloc;

mod arena;
mod util;

#[cfg(feature = "accessor")]
pub mod accessor;
pub mod dma;
pub mod io;
pub mod os;
pub mod task;

pub use self::arena::Arena;

/// A Handle is used to identify resources across privilege (user <-> kernel) boundaries.
pub type Handle = u32;
