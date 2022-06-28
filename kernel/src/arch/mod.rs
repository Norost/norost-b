pub mod amd64;
#[cfg(target_arch = "riscv64gc")]
pub mod riscv;

pub use amd64::*;
