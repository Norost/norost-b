#![no_std]
#![feature(asm)]
#![feature(optimize_attribute)]

#[cfg(feature = "userspace")]
#[macro_use]
pub mod syscall;

#[repr(align(4096))]
#[repr(C)]
pub struct Page([u128; 256]);
