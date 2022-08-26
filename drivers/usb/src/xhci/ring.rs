use crate::dma::Dma;
use core::{
	marker::PhantomData,
	sync::atomic::{self, Ordering},
};
use xhci::ring::trb;

pub struct Ring<T>
where
	T: TrbEntry,
{
	buf: Dma<[[u32; 4]]>,
	enqueue_index: usize,
	_marker: PhantomData<T>,
}

impl<T> Ring<T>
where
	T: TrbEntry,
{
	pub fn new() -> Result<Self, rt::Error> {
		let mut buf = Dma::new_slice(32).unwrap();
		// Set Link TRB at end to estabilish loop
		let link = trb::Link::new()
			.set_ring_segment_pointer(buf.as_phys())
			.set_cycle_bit()
			.into_raw();
		let len = buf.len();
		unsafe { buf.as_mut()[len - 1] = link }
		Ok(Self {
			buf,
			enqueue_index: 0,
			_marker: PhantomData,
		})
	}

	#[cfg_attr(debug_assertions, track_caller)]
	pub fn enqueue(&mut self, item: T) -> EntryId {
		// Reduce monomorphization overhead.
		self.enqueue_inner(item.into_raw())
	}

	#[cfg_attr(debug_assertions, track_caller)]
	fn enqueue_inner(&mut self, mut item: [u32; 4]) -> EntryId {
		let i = self.enqueue_index;
		self.enqueue_index += 1;
		if self.enqueue_index >= self.capacity() {
			self.enqueue_index = 0;
		}

		let b = unsafe { self.buf.as_mut() };

		// Clear cycle bit of the next entry as not to create a loop
		b[self.enqueue_index][3] = 0;
		atomic::fence(Ordering::Release);

		// TODO ensure we don't set the cycle bit before the entry has been fully written.
		// We should try to do this in an efficient way, e.g. a single XMM store is atomic
		// (at least, on all archs with AVX).
		item[3] |= 1; // Set cycle bit
		b[i] = item;
		self.buf.as_phys() + i as u64 * 16
	}

	pub fn as_phys(&self) -> u64 {
		self.buf.as_phys()
	}

	fn capacity(&self) -> usize {
		// -1 to account for link
		self.buf.len() - 1
	}
}

pub struct Full;

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
