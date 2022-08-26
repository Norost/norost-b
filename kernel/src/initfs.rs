use crate::{
	memory::{
		frame::{PageFrameIter, PPN},
		r#virtual::{phys_to_virt, RWX},
		Page,
	},
	object_table::{Error, MemoryObject, Object, PageFlags, QueryIter, SeekFrom, Ticket},
};
use alloc::{boxed::Box, sync::Arc};
use core::{
	slice,
	sync::atomic::{AtomicUsize, Ordering},
};

/// A single file in the init filesystem.
pub struct File {
	data: &'static [u8],
	position: AtomicUsize,
}

unsafe impl MemoryObject for File {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
		let base = unsafe { PPN::from_ptr(self.data.as_ptr() as _) };
		let count = self.physical_pages_len();
		for p in (PageFrameIter { base, count }) {
			if !f(&[p]) {
				break;
			}
		}
	}

	fn physical_pages_len(&self) -> usize {
		Page::min_pages_for_bytes(self.data.len())
	}

	fn page_flags(&self) -> (PageFlags, RWX) {
		(Default::default(), RWX::RX)
	}
}

impl Object for File {
	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		let pos = self
			.position
			.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |p| {
				Some(p.saturating_add(length).min(self.data.len()))
			})
			.unwrap();
		let bottom = self.data.len().min(pos);
		let top = self.data.len().min(pos + length).try_into().unwrap();
		Ticket::new_complete(Ok(self.data[bottom..top].into()))
	}

	fn seek(&self, from: SeekFrom) -> Ticket<u64> {
		let mut pos = None;
		self.position
			.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |p| {
				pos = Some(from.apply(p, self.data.len()));
				pos
			})
			.unwrap();
		Ticket::new_complete(Ok(pos.unwrap().try_into().unwrap()))
	}

	fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
		Some(self)
	}
}

/// The init filesystem root.
pub struct Fs {
	data: &'static [u8],
}

impl Fs {
	fn header(&self) -> nrofs::Header {
		let mut io = self.io();
		nrofs::Header::load(move |o| io.do_io(nrofs::Op::Read(o))).expect("invalid header")
	}

	fn iter(&self) -> impl Iterator<Item = nrofs::Entry> {
		let mut io_it = self.io();
		self.header()
			.iter(move |o| io_it.do_io(o))
			.map(Result::unwrap)
	}

	pub fn find(&self, s: &[u8]) -> Option<Arc<File>> {
		let mut io = self.io();
		let mut buf = [0; 255];
		self.iter()
			.find(|e| e.name(&mut buf, |o| io.do_io(o)).unwrap() == s)
			.map(|e| {
				let start = e.offset(&self.header()).try_into().unwrap();
				let size = e.size().try_into().unwrap();
				Arc::new(File {
					data: &self.data[start..][..size],
					position: 0.into(),
				})
			})
	}

	fn io(&self) -> FsIo {
		FsIo {
			data: self.data,
			cur: 0,
		}
	}
}

struct FsIo {
	data: &'static [u8],
	cur: usize,
}

impl FsIo {
	fn do_io(&mut self, op: nrofs::Op<'_>) -> Result<(), &'static str> {
		let oob = "out of bounds";
		let old_cur = self.cur;
		match op {
			nrofs::Op::Seek(n) => self.cur = n.try_into().map_err(|_| oob)?,
			nrofs::Op::Advance(n) => {
				self.cur = if n > 0 {
					self.cur.checked_add(n.try_into().map_err(|_| oob)?)
				} else {
					self.cur.checked_sub((-n).try_into().map_err(|_| oob)?)
				}
				.ok_or(oob)?
			}
			nrofs::Op::Read(b) => {
				b.copy_from_slice(&self.data[self.cur..].get(..b.len()).ok_or(oob)?);
				self.cur += b.len();
			}
		}
		(self.cur <= self.data.len()).then(|| ()).ok_or_else(|| {
			self.cur = old_cur;
			oob
		})
	}
}

impl Object for Fs {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(if matches!(path, b"" | b"/") {
			let mut io_entry = self.io();
			let mut buf = [0; 255];
			let it = self
				.iter()
				.map(move |e| e.name(&mut buf, |o| io_entry.do_io(o)).unwrap().into());
			Ok(Arc::new(QueryIter::new(it)))
		} else {
			self.find(path).map(|e| e as _).ok_or(Error::DoesNotExist)
		})
	}
}

pub fn post_init(boot: &crate::boot::Info) -> Arc<Fs> {
	// SAFETY: FIXME
	let data = unsafe {
		slice::from_raw_parts(
			phys_to_virt(boot.initfs_ptr.into()),
			boot.initfs_len.try_into().unwrap(),
		)
	};
	Arc::new(Fs { data })
}
