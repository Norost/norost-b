use gui3d::math::int::{Point2, Rect, Size};

pub struct Gpu {
	size: Size,
	shmem: &'static mut [u8],
	shmem_id: u32,
	sync: rt::RefObject<'static>,
}

impl Gpu {
	pub fn new() -> Self {
		let sync = rt::args::handle(b"gpu").expect("gpu undefined");
		let res = {
			let mut b = [0; 8];
			sync.get_meta(b"bin/resolution".into(), (&mut b).into())
				.unwrap();
			ipc_gpu::Resolution::decode(b)
		};
		let size = Size::new(res.x, res.y);

		let shmem_size = size.x as usize * size.y as usize * 3;
		let shmem_size = (shmem_size + 0xfff) & !0xfff;
		let (shmem_obj, _) =
			rt::Object::new(rt::io::NewObject::SharedMemory { size: shmem_size }).unwrap();
		let (shmem, shmem_size) = shmem_obj
			.map_object(None, rt::io::RWX::RW, 0, shmem_size)
			.unwrap();
		let shmem_id = sync
			.share(
				&rt::Object::new(rt::io::NewObject::PermissionMask {
					handle: shmem_obj.as_raw(),
					rwx: rt::io::RWX::R,
				})
				.unwrap()
				.0,
			)
			.expect("failed to share mem with GPU") as _;
		// SAFETY: only we can write to this slice. The other side can go figure.
		let shmem = unsafe { core::slice::from_raw_parts_mut(shmem.as_ptr(), shmem_size) };

		Self { size, shmem, sync, shmem_id }
	}

	pub fn share_buffer(&mut self, share: rt::Object) -> rt::io::Result<u32> {
		self.sync.share(&share).map(|n| n as _)
	}

	pub fn unmap_buffer(&mut self, buffer_id: u32) -> rt::io::Result<()> {
		self.sync
			.set_meta(
				b"bin/buffer/unmap".into(),
				(&buffer_id.to_le_bytes()).into(),
			)
			.map(|_| ())
	}

	pub fn fill(&mut self, rect: Rect, color: [u8; 3]) {
		let t = rect.size();
		assert!(
			t.x <= self.size.x && t.y <= self.size.y,
			"rect out of bounds"
		);
		assert!(t.area() * 3 <= self.shmem.len() as u64, "shmem too small");
		for y in 0..t.y {
			for x in 0..t.x {
				let i = y * t.x + x;
				let s = &mut self.shmem[i as usize * 3..][..3];
				s.copy_from_slice(&color);
			}
		}
		self.sync_rect(None, rect);
	}

	pub fn sync_rect(&mut self, buffer: Option<u32>, rect: Rect) {
		let buffer_id = buffer.unwrap_or(self.shmem_id);
		self.sync
			.write(
				&ipc_gpu::Flush {
					offset: 0,
					stride: rect.size().x,
					origin: ipc_gpu::Point { x: rect.low().x, y: rect.low().y },
					size: ipc_gpu::SizeInclusive { x: rect.size().x as _, y: rect.size().y as _ },
					buffer_id,
				}
				.encode(),
			)
			.unwrap();
	}

	pub fn copy(&mut self, data: &[u8], to: Rect) {
		self.shmem[..data.len()].copy_from_slice(data);
		self.sync_rect(None, to);
	}

	pub fn set_cursor(&mut self, tex: &gui3d::Texture) {
		let r = tex.as_raw();
		self.shmem[..r.len()].copy_from_slice(r);
		let f = |n| u8::try_from(n - 1).unwrap();
		self.sync
			.write(&[0xc5, 0, 0, 0, 0, f(tex.width()), f(tex.height())])
			.unwrap();
	}

	pub fn move_cursor(&mut self, pos: Point2) {
		let [a, b] = (pos.x as u16).to_le_bytes();
		let [c, d] = (pos.y as u16).to_le_bytes();
		self.sync
			.set_meta(b"bin/cursor/pos".into(), (&[a, b, c, d]).into())
			.unwrap();
	}

	pub fn size(&self) -> Size {
		self.size
	}
}
