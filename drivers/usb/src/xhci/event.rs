use alloc::vec::Vec;
use core::{
	marker::PhantomData,
	num::{NonZeroU16, NonZeroU8, NonZeroUsize, Wrapping},
	ptr::NonNull,
	sync::atomic,
};
use driver_utils::dma;
use xhci::{
	registers::InterruptRegisterSet,
	ring::trb::{event::Allowed, Link},
};

#[derive(Debug)]
pub enum Event {
	PortStatusChange { port: NonZeroU8 },
	CommandCompletion { id: u64, slot: NonZeroU8 },
	Transfer { id: u64, slot: NonZeroU8 },
}

pub struct Table {
	ptr: NonNull<SegmentEntry>,
	phys: u64,
	capacity: NonZeroUsize,
	dequeue_ptr: NonNull<[u32; 4]>,
	_marker: PhantomData<SegmentEntry>,
	segments: Vec<Segment>,
}

impl Table {
	pub fn new() -> Result<Self, rt::Error> {
		// xHCI assumes at least 4 KiB page sizes.
		let (ptr, phys, size) = dma::alloc_dma(4096.try_into().unwrap())?;
		let mut s = Self {
			ptr: ptr.cast(),
			phys,
			capacity: (size.get() / 16).try_into().unwrap(),
			dequeue_ptr: NonNull::dangling(),
			_marker: PhantomData,
			segments: Vec::new(),
		};
		s.add_segment();
		s.dequeue_ptr = s.segments[0].ptr;
		Ok(s)
	}

	pub fn add_segment(&mut self) -> Result<(), rt::Error> {
		if self.segments.len() < self.segments.capacity() {
			todo!();
		}
		let (segm, base, size) = Segment::new()?;
		unsafe {
			self.ptr
				.as_ptr()
				.add(self.segments.len())
				.write(SegmentEntry {
					base,
					size: size.get().try_into().unwrap_or(u16::MAX),
					_reserved: [0; 3],
				});
		}
		self.segments.push(segm);
		Ok(())
	}

	pub fn dequeue(&mut self) -> Option<Event> {
		unsafe {
			atomic::fence(atomic::Ordering::Acquire);
			let evt = self.dequeue_ptr.as_ptr().read();

			if evt[3] & 1 == 0 {
				// cycle bit is not set
				return None;
			}

			if let Ok(link) = Link::try_from(evt) {
				let phys = link.ring_segment_pointer();
				assert_eq!(
					self.phys, phys,
					"xHCI controller did not link to start of ring"
				);

				// Find the corresponding virt ptr
				let mut it = self.iter();
				loop {
					let (e, s) = it.next().expect("phys address is not part of any segment");
					// TODO is it safe to assume all xHCI controllers are sane and will start
					// from the base of a segment?
					if e.base == phys {
						drop(it);
						self.dequeue_ptr = s.ptr;
						break;
					}
				}

				return self.dequeue();
			}
			self.dequeue_ptr = NonNull::new(self.dequeue_ptr.as_ptr().add(1)).unwrap();
			Some(match Allowed::try_from(evt).expect("invalid event") {
				Allowed::PortStatusChange(p) => Event::PortStatusChange {
					port: p.port_id().try_into().unwrap(),
				},
				Allowed::Doorbell(_) => todo!(),
				Allowed::MfindexWrap(_) => todo!(),
				Allowed::TransferEvent(c) => Event::Transfer {
					id: c.trb_pointer(),
					slot: c.slot_id().try_into().unwrap(),
				},
				Allowed::HostController(_) => todo!(),
				Allowed::PortStatusChange(_) => todo!(),
				Allowed::BandwidthRequest(_) => todo!(),
				Allowed::CommandCompletion(c) => Event::CommandCompletion {
					id: c.command_trb_pointer(),
					slot: c.slot_id().try_into().unwrap(),
				},
				Allowed::DeviceNotification(_) => todo!(),
			})
		}
	}

	/// # Panics
	///
	/// There are no segments.
	pub fn install(&self, reg: &mut InterruptRegisterSet) {
		let (e, _) = self.get(0).expect("no segments");
		// Program the Interrupter Event Ring Segment Table Size
		reg.erstsz.set(self.segments.len().try_into().unwrap());
		// Program the Interrupter Event Ring Dequeue Pointer
		reg.erdp.set_event_ring_dequeue_pointer(e.base);
		// Program the Interrupter Event Ring Segment Table Base Address
		reg.erstba.set(self.phys);
	}

	fn get(&self, index: usize) -> Option<(&SegmentEntry, &Segment)> {
		let s = self.segments.get(index)?;
		let e = unsafe { &*self.ptr.as_ptr().add(index) };
		Some((e, s))
	}

	fn iter(&self) -> impl Iterator<Item = (&SegmentEntry, &Segment)> {
		self.segments
			.iter()
			.enumerate()
			.map(|(i, s)| unsafe { (&*self.ptr.as_ptr().add(i), s) })
	}
}

#[repr(C)]
struct SegmentEntry {
	base: u64,
	size: u16,
	_reserved: [u16; 3],
}

struct Segment {
	ptr: NonNull<[u32; 4]>,
	_marker: PhantomData<Allowed>,
}

impl Segment {
	fn new() -> Result<(Self, u64, NonZeroUsize), rt::Error> {
		// xHCI assumes at least 4 KiB page sizes.
		let (ptr, phys, size) = dma::alloc_dma(4096.try_into().unwrap())?;
		Ok((
			Self {
				ptr: ptr.cast(),
				_marker: PhantomData,
			},
			phys,
			size,
		))
	}
}
