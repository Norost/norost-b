use crate::dma::Dma;
use core::{
	marker::PhantomData,
	num::{NonZeroUsize, Wrapping},
	ptr::NonNull,
};
use xhci::ring::trb;

pub struct Ring<T>
where
	T: TrbEntry,
{
	buf: Dma<[[u32; 4]]>,
	dequeue_index: Wrapping<usize>,
	enqueue_index: Wrapping<usize>,
	_marker: PhantomData<T>,
}

impl<T> Ring<T>
where
	T: TrbEntry,
{
	pub fn new() -> Result<Self, rt::Error> {
		Ok(Self {
			buf: Dma::new_slice(256).unwrap(),
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
		if self.enqueue_index.0 >= self.buf.len() - 1 {
			let item = trb::Link::new()
				.set_ring_segment_pointer(self.buf.as_phys())
				.into_raw();
			self.enqueue_raw(item)?;
			self.enqueue_index.0 = 0;
		}
		self.enqueue_raw(item)
	}

	fn enqueue_raw(&mut self, mut item: [u32; 4]) -> Result<EntryId, Full> {
		if self.enqueue_index == self.dequeue_index + Wrapping(self.buf.len()) {
			return Err(Full);
		}
		item[3] |= 1; // Set cycle bit
		let i = self.enqueue_index.0;
		// TODO ensure we don't set the cycle bit before the entry has been fully written.
		// We should try to do this in an efficient way, e.g. a single XMM store is atomic.
		unsafe { self.buf.as_mut()[i] = item }
		self.enqueue_index += 1;
		Ok(self.buf.as_phys() + i as u64 * 16)
	}

	pub fn mark_dequeued(&mut self) {
		unsafe {
			// Clear cycle bit
			self.buf.as_mut()[self.dequeue_index.0][3] = 0;
			self.dequeue_index.0 += 1;
			if self.dequeue_index.0 >= self.buf.len() {
				self.dequeue_index.0 = 0;
			}
		}
	}

	pub fn as_phys(&self) -> u64 {
		self.buf.as_phys()
	}
}

pub struct Full;
pub struct Empty;

pub trait TrbEntry: private::Sealed {
	fn into_raw(self) -> [u32; 4];
}

macro_rules! impl_trb {
	($t:ty) => {
		impl private::Sealed for $t {}
		impl TrbEntry for $t {
			fn into_raw(self) -> [u32; 4] {
				self.into_raw()
			}
		}
	};
}
impl_trb!(trb::command::Allowed);
impl_trb!(trb::transfer::Allowed);
impl_trb!(trb::transfer::Normal);

pub type EntryId = u64;

mod private {
	pub trait Sealed {}
}
