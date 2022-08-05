use core::{
	marker::PhantomData,
	num::{NonZeroUsize, Wrapping},
	ptr::NonNull,
};
use driver_utils::dma;
use xhci::ring::trb;

pub struct Ring<T>
where
	T: TrbEntry,
{
	ptr: NonNull<[u32; 4]>,
	phys: u64,
	size: NonZeroUsize,
	dequeue_index: Wrapping<usize>,
	enqueue_index: Wrapping<usize>,
	_marker: PhantomData<T>,
}

impl<T> Ring<T>
where
	T: TrbEntry,
{
	pub fn new() -> Result<Self, rt::Error> {
		// xHCI assumes at least 4 KiB page sizes.
		let (ptr, phys, size) = dma::alloc_dma(4096.try_into().unwrap())?;
		Ok(Self {
			ptr: ptr.cast(),
			phys,
			size,
			dequeue_index: Wrapping(0),
			enqueue_index: Wrapping(0),
			_marker: PhantomData,
		})
	}

	pub fn enqueue(&mut self, item: T) -> Result<EntryId, Full> {
		// Reduce monomorphization overhead.
		self.enqueue_inner(item.into_raw())
	}

	fn enqueue_inner(&mut self, item: [u32; 4]) -> Result<EntryId, Full> {
		if self.enqueue_index.0 >= self.size.get() - 1 {
			let item = trb::Link::new()
				.set_ring_segment_pointer(self.phys)
				.into_raw();
			self.enqueue_raw(item)?;
			self.enqueue_index.0 = 0;
		}
		self.enqueue_raw(item)
	}

	fn enqueue_raw(&mut self, mut item: [u32; 4]) -> Result<EntryId, Full> {
		if self.enqueue_index == self.dequeue_index + Wrapping(self.size.get()) {
			return Err(Full);
		}
		item[3] |= 1; // Set cycle bit
		let i = self.enqueue_index.0;
		// TODO ensure we don't set the cycle bit before the entry has been fully written.
		// We should try to do this in an efficient way, e.g. a single XMM store is atomic.
		unsafe { self.ptr.as_ptr().add(i).write(item) }
		self.enqueue_index += 1;
		Ok(self.phys + i as u64 * 16)
	}

	pub fn mark_dequeued(&mut self) {
		unsafe {
			// Clear cycle bit
			self.ptr
				.as_ptr()
				.add(self.dequeue_index.0)
				.cast::<u32>()
				.add(3)
				.write(0);
			self.dequeue_index.0 += 1;
			if self.dequeue_index.0 >= self.size.get() {
				self.dequeue_index.0 = 0;
			}
		}
	}

	pub fn as_phys(&self) -> u64 {
		self.phys
	}
}

pub struct Full;
pub struct Empty;

pub trait TrbEntry: private::Sealed {
	fn into_raw(self) -> [u32; 4];
}
impl private::Sealed for trb::command::Allowed {}
impl private::Sealed for trb::transfer::Allowed {}
impl TrbEntry for trb::command::Allowed {
	fn into_raw(self) -> [u32; 4] {
		self.into_raw()
	}
}
impl TrbEntry for trb::transfer::Allowed {
	fn into_raw(self) -> [u32; 4] {
		self.into_raw()
	}
}

pub type EntryId = u64;

mod private {
	pub trait Sealed {}
}
