//! # Nora kernel ABI
//!
//! This crate provides structures & functions to facilitate communication with the
//! Nora kernel.

#![cfg_attr(not(test), no_std)]
#![warn(unsafe_op_in_unsafe_fn)]
#![feature(allow_internal_unsafe)]
#![feature(asm_sym)]
#![feature(core_intrinsics)]
#![feature(maybe_uninit_uninit_array)]
#![feature(naked_functions)]
#![feature(optimize_attribute)]
#![feature(slice_ptr_get)]
#![deny(unused)]

pub mod error;
#[cfg(feature = "userspace")]
#[macro_use]
pub mod syscall;
pub mod io;
pub mod object;
pub mod time;

#[repr(align(4096))]
#[repr(C)]
pub struct Page([u8; Self::SIZE]);

impl Page {
	pub const SIZE: usize = 0x1000;
	pub const MASK: usize = 0xfff;

	/// Return the minimum amount of pages to cover the given amount of bytes.
	#[inline]
	pub fn min_pages_for_bytes(bytes: usize) -> usize {
		(bytes + Self::MASK) / Self::SIZE
	}

	/// Return the minimum amount of pages to cover the given amount of bytes in bytes.
	#[inline]
	pub fn align_size(bytes: usize) -> usize {
		(bytes + Self::MASK) & !Self::MASK
	}
}

pub type Handle = u32;

pub type AtomicHandle = core::sync::atomic::AtomicU32;
