use crate::arch::r#virtual;
use crate::memory::{
	r#virtual::{PPN, RWX},
	Page,
};
use crate::scheduler::MemoryObject;
use crate::sync::SpinLock;
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::num::NonZeroUsize;
use core::ops::RangeInclusive;
use core::ptr::NonNull;

#[derive(Debug)]
pub enum MapError {
	Overflow,
	ZeroSize,
	Arch(crate::arch::r#virtual::MapError),
}

#[derive(Debug)]
pub enum UnmapError {}

/// All objects mapped in kernel space. This vector is sorted.
static KERNEL_MAPPED_OBJECTS: SpinLock<
	Vec<(RangeInclusive<NonNull<Page>>, Arc<dyn MemoryObject>)>,
> = SpinLock::new(Vec::new());

pub struct AddressSpace {
	/// The address space mapping used by the MMU
	mmu_address_space: r#virtual::AddressSpace,
	/// All mapped objects. This vector is sorted.
	objects: Vec<(RangeInclusive<NonNull<Page>>, Arc<dyn MemoryObject>)>,
}

impl AddressSpace {
	pub fn new() -> Result<Self, crate::memory::frame::AllocateContiguousError> {
		Ok(Self {
			mmu_address_space: r#virtual::AddressSpace::new()?,
			objects: Default::default(),
		})
	}

	/// Map an object in this current address space in userspace.
	pub fn map_object(
		&mut self,
		base: Option<NonNull<Page>>,
		object: Arc<dyn MemoryObject>,
		rwx: RWX,
		hint_color: u8,
	) -> Result<NonNull<Page>, MapError> {
		let (range, frames, index) = Self::map_object_common(
			&self.objects,
			NonNull::new(Page::SIZE as _).unwrap(),
			base,
			&*object,
		)?;

		unsafe {
			self.mmu_address_space
				.map(
					range.start().as_ptr() as *const _,
					frames.into_vec().into_iter(),
					rwx,
					hint_color,
				)
				.map_err(MapError::Arch)?
		};
		self.objects.insert(index, (range.clone(), object));
		Ok(*range.start())
	}

	/// Map a frame in kernel-space.
	pub fn kernel_map_object(
		base: Option<NonNull<Page>>,
		object: Arc<dyn MemoryObject>,
		rwx: RWX,
	) -> Result<NonNull<Page>, MapError> {
		let mut objects = KERNEL_MAPPED_OBJECTS.auto_lock();

		let (range, frames, index) = Self::map_object_common(
			&objects,
			// TODO don't hardcode base address
			// Current one is between kernel base & identity map base,
			// which gives us 32 TiB of address space, i.e. plenty for now.
			NonNull::new(0xffff_a000_0000_0000usize as _).unwrap(),
			base,
			&*object,
		)?;

		unsafe {
			r#virtual::AddressSpace::kernel_map(
				range.start().as_ptr() as *const _,
				frames.into_vec().into_iter(),
				rwx,
			)
			.map_err(MapError::Arch)?
		};
		objects.insert(index, (range.clone(), object));
		Ok(*range.start())
	}

	fn map_object_common(
		objects: &[(RangeInclusive<NonNull<Page>>, Arc<dyn MemoryObject>)],
		default: NonNull<Page>,
		base: Option<NonNull<Page>>,
		object: &dyn MemoryObject,
	) -> Result<(RangeInclusive<NonNull<Page>>, Box<[PPN]>, usize), MapError> {
		let frames = object.physical_pages();
		let count = NonZeroUsize::new(frames.len()).ok_or(MapError::ZeroSize)?;
		let (base, index) = match base {
			Some(base) => (base, objects.partition_point(|e| e.0.start() < &base)),
			None => Self::find_free_range(objects, count, default)?,
		};
		// FIXME we need to ensure the range doesn't overlap with any other range.
		let end = base
			.as_ptr()
			.wrapping_add(count.get())
			.cast::<u8>()
			.wrapping_sub(1)
			.cast();
		if end < base.as_ptr() {
			return Err(MapError::Overflow);
		}
		let end = NonNull::new(end).unwrap();
		Ok((base..=end, frames, index))
	}

	pub fn unmap_object(
		&mut self,
		base: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		unsafe {
			Self::unmap_object_common(&mut self.objects, base, count)?;
			self.mmu_address_space.unmap(base, count).unwrap();
		}
		Ok(())
	}

	/// # Safety
	///
	/// The memory region may no longer be used after this call.
	pub unsafe fn kernel_unmap_object(
		base: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		let mut objects = KERNEL_MAPPED_OBJECTS.auto_lock();
		unsafe {
			Self::unmap_object_common(&mut objects, base, count)?;
			r#virtual::AddressSpace::kernel_unmap(base, count).unwrap();
		}
		Ok(())
	}

	unsafe fn unmap_object_common(
		objects: &mut Vec<(RangeInclusive<NonNull<Page>>, Arc<dyn MemoryObject>)>,
		base: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		let i = objects.iter().position(|e| e.0.contains(&base)).unwrap();
		let (range, _obj) = &objects[i];
		let end = base
			.as_ptr()
			.wrapping_add(count.get())
			.cast::<u8>()
			.wrapping_sub(1)
			.cast();
		let unmap_range = base..=NonNull::new(end).unwrap();
		if &unmap_range == range {
			objects.remove(i);
		} else {
			todo!("partial unmap");
		}
		Ok(())
	}

	/// Map a virtual address to a physical address.
	pub fn get_physical_address(&self, address: NonNull<()>) -> Option<(usize, RWX)> {
		self.mmu_address_space.get_physical_address(address)
	}

	/// Find a range of free address space.
	fn find_free_range(
		objects: &[(RangeInclusive<NonNull<Page>>, Arc<dyn MemoryObject>)],
		_count: NonZeroUsize,
		default: NonNull<Page>,
	) -> Result<(NonNull<Page>, usize), MapError> {
		// FIXME we need to check if there actually is enough room
		// Try to allocate past the last object, which is very easy & fast to check.
		// Also insert a guard page inbetween.
		objects.last().map_or(Ok((default, 0)), |o| {
			Ok((
				NonNull::new(
					o.0.end()
						.as_ptr()
						.cast::<u8>()
						.wrapping_add(1)
						.cast::<Page>()
						.wrapping_add(1),
				)
				.unwrap(),
				objects.len(),
			))
		})
	}

	pub unsafe fn activate(&self) {
		unsafe { self.mmu_address_space.activate() }
	}

	/// Identity-map a physical frame.
	///
	/// # Returns
	///
	/// `true` if a new mapping has been added, `false` otherwise.
	///
	/// # Panics
	///
	/// `size` must be a multiple of the page size.
	pub fn identity_map(ppn: PPN, size: usize) -> bool {
		assert_eq!(size % Page::SIZE, 0);
		unsafe { r#virtual::add_identity_mapping(ppn.as_phys(), size).is_ok() }
	}

	/// Activate the default address space.
	///
	/// # Safety
	///
	/// There should be no active pointers to any user-space data
	// TODO should we even be using any pointers to user-space data directly?
	pub unsafe fn activate_default() {
		unsafe { r#virtual::AddressSpace::activate_default() }
	}
}
