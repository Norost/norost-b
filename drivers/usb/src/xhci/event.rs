use {
	crate::dma::Dma,
	alloc::vec::Vec,
	core::{marker::PhantomData, ptr::NonNull, sync::atomic},
	xhci::{
		accessor::{marker::ReadWrite, Mapper},
		registers::runtime::{EventRingDequeuePointerRegister, Interrupter},
		ring::trb::event::Allowed,
	},
};

pub struct Table {
	buf: Dma<[SegmentEntry]>,
	dequeue_segment: u8,
	dequeue_index: u16,
	segments: Vec<NonNull<[u32; 4]>>,
	cycle_state_bit_on: bool,
	_marker: PhantomData<(SegmentEntry, Allowed)>,
}

impl Table {
	pub fn new() -> Result<Self, rt::Error> {
		let mut s = Self {
			buf: Dma::new_slice(256).unwrap(),
			dequeue_segment: 0,
			dequeue_index: 0,
			segments: Vec::new(),
			cycle_state_bit_on: true,
			_marker: PhantomData,
		};
		s.add_segment()?;
		Ok(s)
	}

	pub fn add_segment(&mut self) -> Result<(), rt::Error> {
		if self.segments.len() < self.segments.capacity() {
			todo!();
		}
		let (ptr, base) = Dma::<[[u32; 4]]>::new_slice(256)?.into_raw();
		let (ptr, size) = ptr.to_raw_parts();
		unsafe {
			self.buf.as_mut()[self.segments.len()] =
				SegmentEntry { base, size: size.try_into().unwrap(), _reserved: [0; 3] };
		}
		self.segments.push(ptr.cast());
		Ok(())
	}

	pub fn dequeue(&mut self) -> Option<Allowed> {
		atomic::fence(atomic::Ordering::Acquire);
		// Do a raw read as we can't guarantee the controller won't write to
		// the other entries while we hold a reference.
		let evt = unsafe {
			self.segments[usize::from(self.dequeue_segment)]
				.as_ptr()
				.add(usize::from(self.dequeue_index))
				.read()
		};

		if evt[3] & 1 != u32::from(self.cycle_state_bit_on) {
			return None;
		}

		self.dequeue_index += 1;
		let len = unsafe { self.buf.as_ref()[usize::from(self.dequeue_segment)].size };
		if self.dequeue_index >= len {
			self.dequeue_index = 0;
			let next_segm = usize::from(self.dequeue_segment) + 1;
			self.dequeue_segment = if next_segm >= self.segments.len() {
				self.cycle_state_bit_on = !self.cycle_state_bit_on;
				0
			} else {
				next_segm as _
			};
		}

		Some(Allowed::try_from(evt).expect("invalid event"))
	}

	/// # Panics
	///
	/// There are no segments.
	pub fn install(&mut self, mut reg: Interrupter<'_, impl Mapper + Clone, ReadWrite>) {
		// Reset to start
		self.dequeue_index = 0;
		self.dequeue_segment = 0;
		assert!(self.segments.len() > 0, "no segments");
		// Program the Interrupter Event Ring Segment Table Size
		reg.erstsz
			.update_volatile(|c| c.set(self.segments.len().try_into().unwrap()));
		// Program the Interrupter Event Ring Dequeue Pointer
		let mut v = EventRingDequeuePointerRegister::default();
		v.set_event_ring_dequeue_pointer(unsafe { self.buf.as_ref()[0].base });
		reg.erdp.write_volatile(v);
		// Program the Interrupter Event Ring Segment Table Base Address
		reg.erstba.update_volatile(|c| c.set(self.buf.as_phys()));
	}

	pub fn inform(&self, mut reg: Interrupter<'_, impl Mapper + Clone, ReadWrite>) {
		let phys = unsafe { self.buf.as_ref()[usize::from(self.dequeue_segment)].base };
		let phys = phys + u64::from(self.dequeue_index) * 16;
		reg.erdp.update_volatile(|c| {
			c.set_event_ring_dequeue_pointer(phys);
			c.clear_event_handler_busy();
		});
	}
}

#[derive(Default)]
#[repr(C)]
struct SegmentEntry {
	base: u64,
	size: u16,
	_reserved: [u16; 3],
}
