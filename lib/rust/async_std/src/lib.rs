#![no_std]
#![feature(const_trait_impl)]
#![deny(unused_must_use)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

#[macro_use]
pub mod object;
#[cfg(feature = "futures-io")]
pub mod compat;
pub mod env;
pub mod fs;
pub mod io;
pub mod net;
pub mod process;
pub mod queue;
pub mod task;
#[macro_use]
mod macros;

pub use object::{AsyncObject, RefAsyncObject};

#[cfg(all(not(feature = "std"), feature = "rt_default"))]
use rt_default as _;
