//! # Nora kernel ABI
//!
//! This crate provides structures & functions to facilitate communication with the
//! Nora kernel.

#![no_std]
#![warn(unsafe_op_in_unsafe_fn)]
#![feature(allow_internal_unsafe)]
#![feature(asm_sym)]
#![feature(core_intrinsics)]
#![feature(naked_functions)]
#![feature(optimize_attribute)]
#![feature(slice_ptr_get)]

#[cfg(feature = "userspace")]
#[macro_use]
pub mod syscall;

pub mod io;

#[repr(align(4096))]
#[repr(C)]
pub struct Page([u128; 256]);

pub type Handle = u32;
