use crate::arch::r#virtual;
use crate::memory::{
	r#virtual::{PPN, RWX},
	Page,
};
use crate::scheduler::MemoryObject;
use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use core::num::NonZeroUsize;
use core::ops::RangeInclusive;
use core::ptr::NonNull;

#[derive(Debug)]
pub enum MapError {
	Overflow,
	ZeroSize,
	NoFreeVirtualAddressSpace,
	Arch(crate::arch::r#virtual::MapError),
}

pub enum UnmapError {}

pub struct AddressSpace {
	/// The address space mapping used by the MMU
	mmu_address_space: r#virtual::AddressSpace,
	/// All mapped objects. This vector is sorted.
	objects: Vec<(RangeInclusive<NonNull<Page>>, Box<dyn MemoryObject>)>,
}

impl AddressSpace {
	pub fn new() -> Result<Self, crate::memory::frame::AllocateContiguousError> {
		Ok(Self {
			mmu_address_space: r#virtual::AddressSpace::new()?,
			objects: Default::default(),
		})
	}

	pub fn map_object(
		&mut self,
		base: Option<NonNull<Page>>,
		object: Box<dyn MemoryObject>,
		rwx: RWX,
		hint_color: u8,
	) -> Result<NonNull<Page>, MapError> {
		let count = object
			.physical_pages()
			.into_vec()
			.into_iter()
			.flat_map(|f| f)
			.count();
		let count = NonZeroUsize::new(count).ok_or(MapError::ZeroSize)?;
		let (base, base_index) = match base {
			Some(base) => (base, usize::MAX),
			None => self.find_free_range(count)?,
		};
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

		let e = unsafe {
			self.mmu_address_space.map(
				base.as_ptr() as *const _,
				// TODO avoid collect()
				object
					.physical_pages()
					.into_vec()
					.into_iter()
					.flat_map(|f| f)
					.collect::<Vec<_>>()
					.into_iter(),
				rwx,
				hint_color,
			)
		};
		e.map(|()| {
			self.objects.push((base..=end, object));
			self.objects.sort_by(|l, r| l.0.start().cmp(r.0.start()));
			base
		})
		.map_err(MapError::Arch)
	}

	pub fn unmap_object(
		&mut self,
		base: NonNull<Page>,
		count: NonZeroUsize,
	) -> Result<(), UnmapError> {
		let i = self
			.objects
			.iter()
			.position(|e| e.0.contains(&base))
			.unwrap();
		let (range, _obj) = &self.objects[i];
		let end = base
			.as_ptr()
			.wrapping_add(count.get())
			.cast::<u8>()
			.wrapping_sub(1)
			.cast();
		let unmap_range = base..=NonNull::new(end).unwrap();
		if &unmap_range == range {
			self.objects.remove(i);
		} else {
			todo!("partial unmap");
		}

		// Remove from page tables.
		unsafe {
			self.mmu_address_space.unmap(base, count).unwrap();
		}

		Ok(())
	}

	/// Map a virtual address to a physical address.
	pub fn get_physical_address(&self, address: NonNull<()>) -> Option<(usize, RWX)> {
		self.mmu_address_space.get_physical_address(address)
	}

	/// Find a range of free address space.
	fn find_free_range(&mut self, count: NonZeroUsize) -> Result<(NonNull<Page>, usize), MapError> {
		// Try to allocate past the last object, which is very easy & fast to check.
		// Also insert a guard page inbetween.
		self.objects
			.last()
			.map_or(Ok((NonNull::new(Page::SIZE as _).unwrap(), 0)), |o| {
				(Ok((
					NonNull::new(
						o.0.end()
							.as_ptr()
							.cast::<u8>()
							.wrapping_add(1)
							.cast::<Page>()
							.wrapping_add(1),
					)
					.unwrap(),
					self.objects.len(),
				)))
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
