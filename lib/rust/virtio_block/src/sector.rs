use core::{
	mem,
	ops::{Deref, DerefMut},
	slice,
};

/// A single sector. Sectors are 512 bytes large and aligned on a 512 byte boundary.
///
/// They are necessary for the driver to work correctly, as the device reads & writes data in 512
/// byte units.
#[derive(Clone, Copy)]
#[repr(C, align(512))]
pub struct Sector(pub [u8; Self::SIZE]);

impl Sector {
	pub const SIZE: usize = 512;

	/// Return a slice of sectors as a byte array.
	pub fn slice_as_u8<'a>(slice: &'a [Self]) -> &'a [u8] {
		// SAFETY: the size matches in terms of bytes & the address is properly aligned.
		unsafe {
			let ratio = mem::size_of::<Self>() / mem::size_of::<u8>();
			slice::from_raw_parts(slice.as_ptr().cast(), slice.len() * ratio)
		}
	}

	/// Return a mutable slice of sectors as a byte array.
	pub fn slice_as_u8_mut<'a>(slice: &'a mut [Self]) -> &'a mut [u8] {
		// SAFETY: the size matches in terms of bytes & the address is properly aligned.
		unsafe {
			let ratio = mem::size_of::<Self>() / mem::size_of::<u8>();
			slice::from_raw_parts_mut(slice.as_mut_ptr().cast(), slice.len() * ratio)
		}
	}
}

impl Default for Sector {
	fn default() -> Self {
		Self([0; 512])
	}
}

impl AsRef<[Self]> for Sector {
	fn as_ref(&self) -> &[Self] {
		slice::from_ref(self)
	}
}

impl AsMut<[Self]> for Sector {
	fn as_mut(&mut self) -> &mut [Self] {
		slice::from_mut(self)
	}
}

impl AsRef<[u8]> for Sector {
	fn as_ref(&self) -> &[u8] {
		&self.0[..]
	}
}

impl AsMut<[u8]> for Sector {
	fn as_mut(&mut self) -> &mut [u8] {
		&mut self.0[..]
	}
}

impl Deref for Sector {
	type Target = [u8; 512];

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for Sector {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}
