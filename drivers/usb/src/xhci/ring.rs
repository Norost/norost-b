use {
	crate::dma::Dma,
	core::{
		marker::PhantomData,
		sync::atomic::{self, Ordering},
	},
	xhci::ring::trb,
};

pub struct Ring<T>
where
	T: TrbEntry,
{
	buf: Dma<[[u32; 4]]>,
	enqueue_index: usize,
	cycle_bit: bool,
	_marker: PhantomData<T>,
}

impl<T> Ring<T>
where
	T: TrbEntry,
{
	/// # Note
	///
	/// Ring Cycle State must be cleared.
	pub fn new() -> Result<Self, rt::Error> {
		let mut buf = Dma::new_slice(32).unwrap();
		// Set Link TRB at end to estabilish loop
		let link = trb::Link::new()
			.set_ring_segment_pointer(buf.as_phys())
			// Toggle cycle bit every time we wrap around.
			.set_toggle_cycle()
			.set_cycle_bit()
			.into_raw();
		let len = buf.len();
		unsafe { buf.as_mut()[len - 1] = link }
		Ok(Self { buf, enqueue_index: 0, cycle_bit: true, _marker: PhantomData })
	}

	pub fn enqueue(&mut self, item: T) -> EntryId {
		// Reduce monomorphization overhead.
		self.enqueue_inner(item.into_raw())
	}

	fn enqueue_inner(&mut self, mut item: [u32; 4]) -> EntryId {
		let (i, c) = (self.enqueue_index, self.cycle_bit);
		let cap = self.capacity();
		let b = unsafe { self.buf.as_mut() };

		self.enqueue_index += 1;
		if self.enqueue_index >= cap {
			trace!("ring wrap");
			b[self.enqueue_index][3] &= !1;
			b[self.enqueue_index][3] |= u32::from(c);
			self.enqueue_index = 0;
			self.cycle_bit = !self.cycle_bit;
		}

		// TODO ensure we don't set the cycle bit before the entry has been fully written.
		// We should try to do this in an efficient way, e.g. a single XMM store is atomic
		// (at least, on all archs with AVX).
		item[3] &= !1;
		item[3] |= u32::from(c);
		atomic::fence(Ordering::Release);
		b[i] = item;
		let p = self.buf.as_phys() + i as u64 * 16;
		trace!("ring enqueue {:x} cb {}", p, c);
		p
	}

	pub fn as_phys(&self) -> u64 {
		self.buf.as_phys()
	}

	fn capacity(&self) -> usize {
		// -1 to account for link
		self.buf.len() - 1
	}
}

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
