use {core::mem, norostb_rt as rt};

pub struct PortIo(rt::Object);

macro_rules! op {
	($ty:ident $in:ident $out:ident) => {
		pub fn $in(&self, addr: u16) -> $ty {
			let mut b = [0; mem::size_of::<$ty>()];
			self.0.seek(rt::io::SeekFrom::Start(addr.into())).unwrap();
			self.0.read(&mut b).unwrap();
			<$ty>::from_le_bytes(b)
		}

		pub fn $out(&self, addr: u16, value: $ty) {
			self.0.seek(rt::io::SeekFrom::Start(addr.into())).unwrap();
			self.0.write(&value.to_le_bytes()).unwrap();
		}
	};
}

impl PortIo {
	pub fn new() -> rt::io::Result<Self> {
		rt::io::file_root()
			.unwrap_or_else(|| todo!())
			.open(b"portio/map")
			.map(Self)
	}

	op!(u8 in8 out8);
	op!(u16 in16 out16);
	op!(u32 in32 out32);
}
