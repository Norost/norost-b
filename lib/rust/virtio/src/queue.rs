//! Implementation of **split** virtqueues.

use core::convert::{TryFrom, TryInto};
use core::fmt;
use core::mem;
use core::ptr::NonNull;
use core::slice;
use core::sync::atomic::{self, Ordering};
use endian::{u16le, u32le, u64le};

#[repr(C)]
struct Descriptor {
	address: u64le,
	length: u32le,
	flags: u16le,
	next: u16le,
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
	free_descriptors: [u16; 8],
	free_count: u16,
	descriptors: NonNull<Descriptor>,
	pub available: NonNull<Avail>,
	pub used: NonNull<Used>,
	notify_offset: u16,
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
	pub fn new(
		config: &'a super::pci::CommonConfig,
		index: u16,
		max_size: u16,
		msix: Option<u16>,
		dma_alloc: impl FnOnce(usize) -> Result<(NonNull<()>, usize), ()>,
	) -> Result<Self, OutOfMemory> {
		// TODO ensure max_size is a power of 2
		let size = u16::from(config.queue_size.get()).min(max_size) as usize;
		let desc_size = mem::size_of::<Descriptor>() * size;
		let avail_size = mem::size_of::<AvailHead>()
			+ mem::size_of::<AvailElement>() * size
			+ mem::size_of::<AvailTail>();
		let used_size = mem::size_of::<UsedHead>()
			+ mem::size_of::<UsedElement>() * size
			+ mem::size_of::<UsedTail>();

		let align = |s| (s + 0xfff) & !0xfff;

		let (mem, phys) = dma_alloc(align(desc_size + avail_size) + align(used_size)).unwrap();
		let mem = mem.cast::<u8>();

		let descriptors = mem.cast();
		let available =
			NonNull::new(mem.cast::<u8>().as_ptr().wrapping_add(desc_size).cast()).unwrap();
		let used = unsafe {
			NonNull::<Used>::new_unchecked(mem.as_ptr().add(align(desc_size + avail_size)).cast())
		};

		unsafe {
			for i in 0..size {
				*used.as_ptr().cast::<UsedHead>().add(1).cast::<u16>().add(i) = 0xffff
			}
		}

		let mut free_descriptors = [0; 8];
		for (i, u) in free_descriptors.iter_mut().enumerate() {
			*u = i as u16;
		}
		let free_descriptors = [5, 7, 6, 0, 1, 3, 2, 4];
		let free_count = 8;

		let d_phys = phys;
		let a_phys = phys + desc_size;
		let u_phys = phys + align(desc_size + avail_size);

		config.queue_select.set(index.into());
		config.queue_descriptors.set((d_phys as u64).into());
		config.queue_driver.set((a_phys as u64).into());
		config.queue_device.set((u_phys as u64).into());
		config.queue_size.set((size as u16).into());
		config.queue_enable.set(1.into());

		let notify_offset = config.queue_notify_off.get().into();

		msix.map(|msix| config.queue_msix_vector.set(msix.into()));

		Ok(Queue {
			_config: config,
			mask: size as u16 - 1,
			last_used: 0,
			free_descriptors,
			free_count,
			descriptors,
			available,
			used,
			notify_offset,
		})
	}

	/// Convert an iterator of `(address, data)` into a linked list of descriptors and put it in the
	/// available ring.
	///
	/// Two callback functions can be specified:
	///
	/// * The first will return a descriptor associated with each entry in the iterator
	///
	/// * The second will return the descriptor, physical address and size associated with each
	///   buffer that may be collected.
	pub fn send<I>(
		&mut self,
		iterator: I,
		mut used: Option<&mut dyn FnMut(u16)>,
		callback: impl FnMut(u16, u64, u32),
	) -> Result<(), NoBuffers>
	where
		I: ExactSizeIterator<Item = (u64, u32, bool)>,
	{
		let count = iterator.len().try_into().unwrap();
		if count == 0 {
			// TODO is this really the right thing to do?
			return Ok(());
		}

		if self.free_count < count {
			self.collect_used(callback);
			(self.free_count < count).then(|| ()).ok_or(NoBuffers)?;
		}

		let desc = descriptors_table!(self);
		let (avail_head, avail_ring) = available_ring!(self);

		let mut head = u16le::from(0);
		let mut prev_next = &mut head;
		let mut iterator = iterator.peekable();
		let mut free_count = self.free_count;
		while let Some((address, length, write)) = iterator.next() {
			free_count = free_count.checked_sub(1).ok_or(NoBuffers)?;
			let i = usize::from(self.free_descriptors[usize::from(free_count)]);
			desc[i].address = u64le::from(u64::try_from(address).expect("Address out of bounds"));
			desc[i].length = u32le::from(u32::try_from(length).expect("Length too large"));
			desc[i].flags = u16le::from(u16::from(write) * Descriptor::WRITE);
			desc[i].flags |= u16le::from(u16::from(iterator.peek().is_some()) * Descriptor::NEXT);
			used.as_mut().map(|f| f(i as u16));
			*prev_next = u16le::from(i as u16);
			prev_next = &mut desc[i].next;
		}
		self.free_count = free_count;

		avail_ring[usize::from(u16::from(avail_head.index) & self.mask)].index = head;
		atomic::fence(Ordering::AcqRel);
		avail_head.index = u16::from(avail_head.index).wrapping_add(1).into();

		Ok(())
	}

	/// Collect used buffers from the device and add them to the free_descriptors list.
	///
	/// A callback function can be specified which will return the descriptor, physical address
	/// and size associated with each buffer.
	///
	/// # Returns
	///
	/// The amount of buffers collected.
	pub fn collect_used(&mut self, mut callback: impl FnMut(u16, u64, u32)) -> usize {
		atomic::fence(Ordering::Acquire);
		let (head, ring) = used_ring!(self);
		let table = descriptors_table!(self);

		let mut index @ last = self.last_used;
		let head_index = u16::from(head.index);
		//callback(head_index, index.into(), u32::MAX - 1);

		while index != head_index {
			// TODO maybe we should use unwrap?
			let mut descr_index = u32::from(ring[usize::from(index & self.mask)].index) as u16;
			loop {
				assert_ne!(descr_index, u16::MAX);
				let descr = &table[usize::from(descr_index)];
				callback(descr_index, descr.address.into(), descr.length.into());
				self.free_descriptors[usize::from(self.free_count)] = descr_index;
				self.free_count += 1;
				if u16::from(descr.flags) & Descriptor::NEXT > 0 {
					descr_index = descr.next.into();
				} else {
					break;
				}
			}
			index = index.wrapping_add(1);
		}
		self.last_used = index;
		usize::from(head_index.wrapping_sub(last))
	}

	/// Wait for any used buffers to appear in the queue, which is useful for polling
	/// a device for readiness.
	///
	/// An optional wait function can be specified to do other work instead of idly
	/// wasting cycles.
	pub fn wait_for_used(
		&mut self,
		mut callback: impl FnMut(u16, u64, u32),
		mut wait_fn: impl FnMut(),
	) {
		while usize::from(self.free_count) != self.free_descriptors.len()
			&& self.collect_used(&mut callback) == 0
		{
			//callback(self.free_count, self.free_descriptors.len() as u64, u32::MAX);
			wait_fn();
		}
	}

	/// Return the offset relative to the notify address to flush this queue.
	pub fn notify_offset(&self) -> u16 {
		self.notify_offset
	}
}

pub struct OutOfMemory;

impl fmt::Debug for OutOfMemory {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "No free DMA memory")
	}
}

pub struct NoBuffers;

impl fmt::Debug for NoBuffers {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "No free buffers")
	}
}
