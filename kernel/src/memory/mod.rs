pub mod frame;
pub mod r#virtual;

#[cfg(target_arch = "x86_64")]
pub struct Page([u8; Self::SIZE]);

impl Page {
	pub const SIZE: usize = 4096;
}
