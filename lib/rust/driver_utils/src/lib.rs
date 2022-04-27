//! # Common driver utilities
//!
//! This crate has a collection of types that are commonly in drivers.

#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_slice)]
#![feature(new_uninit)]
#![feature(norostb)]
#![feature(read_buf)]

mod arena;

pub mod io;

pub use self::arena::Arena;

/// A Handle is used to identify resources across privilege (user <-> kernel) boundaries.
pub type Handle = u32;
