use {
	super::{fixed_bitmap::FixedBitmap, PPNBox, Page, PageFrameIter, PPN},
	crate::{
		boot,
		memory::r#virtual::RWX,
		object_table::{Error, Object, PageFlags, Root, Ticket},
		scheduler::MemoryObject,
		sync::SpinLock,
	},
	alloc::{boxed::Box, string::ToString, sync::Arc},
	core::{num::NonZeroUsize, str},
};

static DMA: SpinLock<FixedBitmap> = SpinLock::new(Default::default());

/// A physically contiguous range of pages
struct DmaFrame {
	base: PPN,
	count: PPNBox,
}

impl DmaFrame {
	fn new(count: NonZeroUsize) -> Result<Self, Error> {
		let base = DMA
			.auto_lock()
			.pop_range(count)
			.ok_or(Error::CantCreateObject)?;
		unsafe {
			base.as_ptr().write_bytes(0, count.get());
		}
		let count = count.get().try_into().unwrap();
		Ok(Self { base, count })
	}
}

impl Object for DmaFrame {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(match path {
			b"size" => Ok(Arc::new(Size(
				usize::try_from(self.count).unwrap() * Page::SIZE,
			))),
			b"phys" => Ok(Arc::new(Phys(self.base))),
			_ => Err(Error::InvalidData),
		})
	}

	fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
		Some(self)
	}
}

unsafe impl MemoryObject for DmaFrame {
	fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
		for p in (0..self.count).map(|i| self.base.skip(i)) {
			if !f(&[p]) {
				break;
			}
		}
	}

	fn physical_pages_len(&self) -> usize {
		self.count.try_into().unwrap()
	}

	fn page_flags(&self) -> (PageFlags, RWX) {
		(Default::default(), RWX::RWX)
	}
}

impl Drop for DmaFrame {
	fn drop(&mut self) {
		let mut iter = PageFrameIter { base: self.base, count: self.count.try_into().unwrap() };
		unsafe {
			super::deallocate(self.count.try_into().unwrap(), || iter.next().unwrap()).unwrap();
		}
	}
}

struct Size(usize);

impl Object for Size {
	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		let s = self.0.to_string();
		Ticket::new_complete(
			(s.len() <= length)
				.then(|| s.into_bytes().into())
				.ok_or(Error::InvalidData),
		)
	}
}

struct Phys(PPN);

impl Object for Phys {
	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		let s = self.0.as_phys().to_string();
		Ticket::new_complete(
			(s.len() <= length)
				.then(|| s.into_bytes().into())
				.ok_or(Error::InvalidData),
		)
	}
}

struct Dma;

impl Object for Dma {
	fn create(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete((|| {
			let path = str::from_utf8(path).map_err(|_| Error::InvalidData)?;
			let n = path
				.parse::<NonZeroUsize>()
				.map_err(|_| Error::InvalidData)?;
			let n = NonZeroUsize::new(Page::min_pages_for_bytes(n.get())).unwrap();
			DmaFrame::new(n).map(|o| Arc::new(o) as Arc<dyn Object>)
		})())
	}
}

/// # Safety
///
/// This may only be called once at boot time.
pub(super) unsafe fn init(memory_regions: &mut [boot::MemoryRegion]) {
	let mut dma = DMA.isr_lock();
	for mr in memory_regions.iter_mut() {
		dma.add_region(mr);
	}
}

pub(super) fn post_init(root: &Root) {
	let dma = Arc::new(Dma) as Arc<dyn Object>;
	root.add(*b"dma", Arc::downgrade(&dma));
	let _ = Arc::into_raw(dma);
}
