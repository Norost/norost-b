#![no_std]
#![feature(const_trait_impl)]
#![deny(unused_must_use)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

#[macro_use]
pub mod object;
pub mod fs;
pub mod io;
pub mod net;
pub mod process;
pub mod queue;
pub mod task;
#[macro_use]
mod macros;

use object::AsyncObject;
