#![no_std]
#![feature(asm)]
#![feature(optimize_attribute)]
#![feature(slice_ptr_get)]

#[cfg(feature = "userspace")]
#[macro_use]
pub mod syscall;

pub mod object_table;

#[repr(align(4096))]
#[repr(C)]
pub struct Page([u128; 256]);
