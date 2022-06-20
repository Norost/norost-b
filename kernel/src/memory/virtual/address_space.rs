use crate::{
	arch::r#virtual,
	memory::{
		r#virtual::{PPN, RWX},
		Page,
	},
	{object_table::MemoryMap, scheduler::MemoryObject, sync::SpinLock},
};
use alloc::{sync::Arc, vec::Vec};
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
	pub fn new() -> Result<Self, crate::memory::frame::AllocateError> {
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
		let (range, index) = Self::map_object_common(
			&self.objects,
			NonNull::new(Page::SIZE as _).unwrap(),
			base,
			&*object,
		)?;

		unsafe {
			let mut f =
				self.mmu_address_space
					.map(range.start().as_ptr() as *const _, rwx, hint_color);
			object.physical_pages(&mut |p| {
				for &p in p.iter() {
					f(p).unwrap_or_else(|e| todo!("{:?}", MapError::Arch(e)))
				}
			});
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
		// FIXME this will deadlock because there is now a circular dependency
		// on the heap allocator
		let mut objects = KERNEL_MAPPED_OBJECTS.auto_lock();

		let (range, index) = Self::map_object_common(
			&objects,
			// TODO don't hardcode base address
			// Current one is between kernel base & identity map base,
			// which gives us 32 TiB of address space, i.e. plenty for now.
			NonNull::new(0xffff_a000_0000_0000usize as _).unwrap(),
			base,
			&*object,
		)?;

		unsafe {
			let mut f =
				r#virtual::AddressSpace::kernel_map(range.start().as_ptr() as *const _, rwx);
			object.physical_pages(&mut |p| {
				for &p in p.iter() {
					f(p).unwrap_or_else(|e| todo!("{:?}", MapError::Arch(e)))
				}
			});
		};
		objects.insert(index, (range.clone(), object));
		Ok(*range.start())
	}

	fn map_object_common(
		objects: &[(RangeInclusive<NonNull<Page>>, Arc<dyn MemoryObject>)],
		default: NonNull<Page>,
		base: Option<NonNull<Page>>,
		object: &dyn MemoryObject,
	) -> Result<(RangeInclusive<NonNull<Page>>, usize), MapError> {
		let frames_len = object.physical_pages_len();
		let count = NonZeroUsize::new(frames_len).ok_or(MapError::ZeroSize)?;
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
		Ok((base..=end, index))
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
		let obj = unsafe {
			r#virtual::AddressSpace::kernel_unmap(base, count).unwrap();
			Self::unmap_object_common(&mut objects, base, count)?
		};
		drop(objects); // Release now to avoid deadlock
		drop(obj);
		Ok(())
	}

	unsafe fn unmap_object_common(
		objects: &mut Vec<(RangeInclusive<NonNull<Page>>, Arc<dyn MemoryObject>)>,
		base: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<Option<Arc<dyn MemoryObject>>, UnmapError> {
		let i = objects.iter().position(|e| e.0.contains(&base)).unwrap();
		let (range, _) = &objects[i];
		let end = base
			.as_ptr()
			.wrapping_add(count.get())
			.cast::<u8>()
			.wrapping_sub(1)
			.cast();
		let unmap_range = base..=NonNull::new(end).unwrap();
		if &unmap_range == range {
			Ok(Some(objects.remove(i).1))
		} else {
			todo!("partial unmap");
		}
	}

	/// Create a [`MemoryMap`] from an address range.
	///
	/// There may not be any holes in the given range.
	pub fn create_memory_map(&mut self, range: RangeInclusive<NonNull<Page>>) -> Option<MemoryMap> {
		let (s, e) = (range.start().as_ptr(), range.end().as_ptr());
		if !s.is_aligned() || !e.wrapping_byte_add(1).is_aligned() || s > e {
			// TODO return an error instead of just None.
			return None;
		}
		let mut it = self.objects.iter();
		let mut obj = Vec::new();
		// TODO use binary search to find start and end
		let (start_offset, mut last_end, has_end) = 'l: loop {
			for (r, o) in &mut it {
				if r.contains(range.start()) {
					obj.push(o.clone());
					break 'l (
						// FIXME r.start() may not correspond with object start
						r.start().as_ptr() as usize - range.start().as_ptr() as usize,
						r.end().as_ptr(),
						r.contains(r.end()),
					);
				}
			}
			return None;
		};
		if !has_end {
			'g: loop {
				for (r, o) in &mut it {
					if last_end.wrapping_add(1) != r.start().as_ptr() {
						return None;
					}
					last_end = r.end().as_ptr();
					obj.push(o.clone());
					if r.contains(range.end()) {
						// FIXME ditto but end
						break 'g;
					}
				}
				return None;
			}
		}
		let total_size = range.end().as_ptr() as usize - range.start().as_ptr() as usize + 1;
		debug_assert_eq!(start_offset % Page::SIZE, 0);
		debug_assert_eq!(total_size % Page::SIZE, 0);
		Some(MemoryMap::new(
			obj.into(),
			start_offset / Page::SIZE,
			total_size / Page::SIZE,
		))
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
	pub fn identity_map(ppn: PPN, size: usize) -> Result<bool, IdentityMapError> {
		assert_eq!(size % Page::SIZE, 0);
		unsafe {
			r#virtual::add_identity_mapping(ppn.as_phys(), size).map_err(IdentityMapError::Arch)
		}
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

#[derive(Debug)]
pub enum IdentityMapError {
	Arch(r#virtual::IdentityMapError),
}
