//! Implementation of **split** virtqueues.

use {
	crate::{PhysAddr, PhysRegion},
	core::{
		cell::Cell,
		convert::{TryFrom, TryInto},
		fmt, mem,
		ptr::NonNull,
		slice,
		sync::atomic::{self, Ordering},
	},
	endian::{u16le, u32le},
};

#[repr(C)]
struct Descriptor {
	address: Cell<PhysAddr>,
	length: Cell<u32le>,
	flags: Cell<u16le>,
	next: Cell<u16le>,
}

impl Descriptor {
	const NEXT: u16 = 0x1;
	const WRITE: u16 = 0x2;
	#[allow(dead_code)]
	const AVAIL: u16 = 1 << 7;
	#[allow(dead_code)]
	const USED: u16 = 1 << 15;
}

pub struct Avail;

#[repr(C)]
struct AvailHead {
	flags: u16le,
	index: u16le,
}

#[repr(C)]
struct AvailElement {
	index: u16le,
}

#[repr(C)]
/// Only for VIRTIO_F_EVENT_IDX
struct AvailTail {
	used_event: u16le,
}

pub struct Used;

#[repr(C)]
struct UsedHead {
	flags: u16le,
	index: u16le,
}

#[repr(C)]
struct UsedElement {
	index: u32le,
	length: u32le,
}

#[repr(C)]
struct UsedTail {
	avail_event: u16le,
}

pub struct Queue<'a> {
	_config: &'a super::pci::CommonConfig,
	mask: u16,
	last_used: u16,
	alloc: DescriptorAlloc,
	descriptors: NonNull<Descriptor>,
	available: NonNull<Avail>,
	used: NonNull<Used>,
	notify_offset: u16,
}

struct DescriptorAlloc {
	free_head: u16,
	// A separate counter is more efficient than (ab)using the flags field as it avoids many
	// loads/stores.
	free_count: u16,
}

/// Returns the available head & ring.
///
/// This is implemented as a macro because Rust isn't quite advanced enough yet.
macro_rules! available_ring {
	($self:ident) => {
		unsafe { return_ring::<Avail, AvailHead, AvailElement>(&mut $self.available, $self.mask) }
	};
}

/// Returns the used head & ring.
///
/// This is implemented as a macro because Rust isn't quite advanced enough yet.
macro_rules! used_ring {
	($self:ident) => {
		unsafe { return_ring::<Used, UsedHead, UsedElement>(&mut $self.used, $self.mask) }
	};
}

/// Returns the descriptors table.
///
/// This is implemented as a macro because Rust isn't quite advanced enough yet.
macro_rules! descriptors_table {
	($self:ident) => {
		unsafe { return_table::<Descriptor>(&mut $self.descriptors, $self.mask) }
	};
}

/// Returns the head & ring.
unsafe fn return_ring<'s, R, H, E>(ptr: &'s mut NonNull<R>, mask: u16) -> (&'s mut H, &'s mut [E]) {
	let size = usize::from(mask) + 1;
	let head = &mut *ptr.as_ptr().cast::<H>();
	let ring = ptr.as_ptr().cast::<u8>().add(mem::size_of::<H>());
	let ring = slice::from_raw_parts_mut(ring.cast(), size);
	(head, ring)
}

/// Returns the table
unsafe fn return_table<'s, T>(ptr: &'s mut NonNull<T>, mask: u16) -> &'s mut [T] {
	let size = usize::from(mask) + 1;
	slice::from_raw_parts_mut(ptr.as_ptr(), size)
}

impl<'a> Queue<'a> {
	/// Create a new split virtqueue and attach it to the device.
	///
	/// The size must be a power of 2.
	pub fn new<DmaError>(
		config: &'a super::pci::CommonConfig,
		index: u16,
		max_size: u16,
		msix: Option<u16>,
		dma_alloc: impl FnOnce(usize, usize) -> Result<(NonNull<()>, PhysAddr), DmaError>,
	) -> Result<Self, NewQueueError<DmaError>> {
		// TODO ensure max_size is a power of 2
		let size = usize::from(u16::from(config.queue_size.get()).min(max_size));
		let desc_size = mem::size_of::<Descriptor>() * size;
		let avail_size = mem::size_of::<AvailHead>()
			+ mem::size_of::<AvailElement>() * size
			+ mem::size_of::<AvailTail>();
		let used_size = mem::size_of::<UsedHead>()
			+ mem::size_of::<UsedElement>() * size
			+ mem::size_of::<UsedTail>();

		let align = |s| (s + 0xfff) & !0xfff;

		let (mem, phys) = dma_alloc(align(desc_size + avail_size) + align(used_size), 4096)
			.map_err(NewQueueError::DmaError)?;
		let mem = mem.cast::<u8>();

		let descriptors = mem.cast();
		let available =
			NonNull::new(mem.cast::<u8>().as_ptr().wrapping_add(desc_size).cast()).unwrap();
		let used = unsafe {
			NonNull::<Used>::new_unchecked(mem.as_ptr().add(align(desc_size + avail_size)).cast())
		};

		let d_phys = phys;
		let a_phys = phys + u64::try_from(desc_size).unwrap();
		let u_phys = phys + u64::try_from(align(desc_size + avail_size)).unwrap();

		config.queue_select.set(index.into());
		config.queue_descriptors.set(d_phys);
		config.queue_driver.set(a_phys);
		config.queue_device.set(u_phys);
		config.queue_size.set((size as u16).into());
		config.queue_enable.set(1.into());

		let notify_offset = config.queue_notify_off.get().into();

		msix.map(|msix| config.queue_msix_vector.set(msix.into()));

		let mut q = Queue {
			_config: config,
			mask: size as u16 - 1,
			last_used: 0,
			alloc: DescriptorAlloc { free_head: 0, free_count: 0 },
			descriptors,
			available,
			used,
			notify_offset,
		};

		(0..size).for_each(|i| q.alloc.push_free_descr(descriptors_table!(q), i as _));

		Ok(q)
	}

	/// Convert an iterator of `(address, data)` into a linked list of descriptors and put it in the
	/// available ring.
	///
	/// # Panics
	///
	/// The iterator must return at least one element, otherwise no descriptors can actually
	/// be sent.
	pub fn send<I>(&mut self, iterator: I) -> Result<Token, NoBuffers>
	where
		I: ExactSizeIterator<Item = (PhysAddr, u32, bool)>,
	{
		let count = iterator.len().try_into().unwrap();
		assert!(count != 0, "expected at least one element");

		if self.alloc.free_count < count {
			return Err(NoBuffers);
		}

		let (avail_head, avail_ring) = available_ring!(self);
		let desc = descriptors_table!(self);

		let head = Cell::new(u16le::from(0));
		let mut prev_next = &head;
		let mut iterator = iterator.peekable();
		while let Some((address, length, write)) = iterator.next() {
			let i = usize::from(self.alloc.pop_free_descr(desc).unwrap());
			desc[i].address.set(address);
			desc[i]
				.length
				.set(u32::try_from(length).expect("Length too large").into());
			desc[i].flags.set(u16le::from(
				u16::from(write) * Descriptor::WRITE
					| u16::from(iterator.peek().is_some()) * Descriptor::NEXT,
			));
			prev_next.set(u16le::from(i as u16));
			prev_next = &desc[i].next;
		}

		avail_ring[usize::from(u16::from(avail_head.index) & self.mask)].index = head.get();
		atomic::fence(Ordering::AcqRel);
		avail_head.index = u16::from(avail_head.index).wrapping_add(1).into();

		Ok(Token(head.get()))
	}

	/// Collect used buffers from the device and add them to the free_descriptors list.
	///
	/// The callback is called once for each returned head descriptor.
	///
	/// # Returns
	///
	/// The amount of buffers collected.
	#[allow(unreachable_code, dead_code, unused)]
	pub fn collect_used(&mut self, mut callback: impl FnMut(Token, PhysRegion)) -> usize {
		atomic::fence(Ordering::Acquire);
		let (head, ring) = used_ring!(self);
		let table = descriptors_table!(self);

		let mut index @ last = self.last_used;
		let head_index = u16::from(head.index);

		while index != head_index {
			// TODO maybe we should use unwrap?
			let mut descr_index = u32::from(ring[usize::from(index & self.mask)].index) as u16;
			let base = table[usize::from(descr_index)].address.get().into();
			let size = table[usize::from(descr_index)].length.get().into();
			callback(Token(descr_index.into()), PhysRegion { base, size });
			loop {
				let descr = &table[usize::from(descr_index)];
				let (flags, next) = (descr.flags.get(), descr.next.get());
				self.alloc.push_free_descr(table, descr_index);
				if Descriptor::NEXT & flags > 0 {
					debug_assert_ne!(descr_index, next, "cycle | {}", self.alloc.free_count);
					descr_index = next.into();
				} else {
					break;
				}
			}
			index = index.wrapping_add(1);
		}
		self.last_used = index;
		usize::from(head_index.wrapping_sub(last))
	}

	/// Return the offset relative to the notify address to flush this queue.
	pub fn notify_offset(&self) -> u16 {
		self.notify_offset
	}
}

impl DescriptorAlloc {
	/// Get a free descriptor if any are available
	fn pop_free_descr(&mut self, table: &[Descriptor]) -> Option<u16> {
		self.free_count.checked_sub(1).map(|c| {
			self.free_count = c;
			let head = self.free_head;
			self.free_head = table[usize::from(head)].next.get().into();
			head
		})
	}

	/// Add a free descriptor
	fn push_free_descr(&mut self, table: &[Descriptor], descr: u16) {
		let head = mem::replace(&mut self.free_head, descr);
		table[usize::from(descr)].next.set(head.into());
		self.free_count += 1;
	}
}

pub struct NoBuffers;

impl fmt::Debug for NoBuffers {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "No free buffers")
	}
}

#[derive(Debug)]
pub enum NewQueueError<DmaError> {
	DmaError(DmaError),
}

/// A token for a single descriptor that has been sent to the device.
///
/// A token must not be reused after it is returned from [`Queue::collect_used`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Token(u16le);
