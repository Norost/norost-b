pub mod amd64;

pub use amd64::*;

pub unsafe fn init() {
	amd64::init();
}
