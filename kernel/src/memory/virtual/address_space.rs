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
	/// All mapped objects
	objects: Vec<(RangeInclusive<NonNull<Page>>, Box<dyn MemoryObject>)>,
	/// All free virtual addresses. Used to speed up allocation.
	///
	/// The value refers to the amount of free *pages*, not bytes!
	free_virtual_addresses: BTreeMap<NonNull<Page>, NonZeroUsize>,
}

impl AddressSpace {
	pub fn new() -> Result<Self, crate::memory::frame::AllocateContiguousError> {
		Ok(Self {
			mmu_address_space: r#virtual::AddressSpace::new()?,
			objects: Default::default(),
			// TODO the available range is arch-defined.
			//free_virtual_addresses: [(NonNull::dangling(), NonZeroUsize::new(4096).unwrap())].into(),
			free_virtual_addresses: [(
				NonNull::new(0x1000_0000 as *mut _).unwrap(),
				NonZeroUsize::new(4096).unwrap(),
			)]
			.into(),
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
		let base = base
			.ok_or(())
			.or_else(|()| self.allocate_virtual_address_range(count))
			.map_err(|NoFreeVirtualAddressSpace| MapError::NoFreeVirtualAddressSpace)?;
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

	/// Allocate a range of virtual address space.
	pub fn allocate_virtual_address_range(
		&mut self,
		count: NonZeroUsize,
	) -> Result<NonNull<Page>, NoFreeVirtualAddressSpace> {
		for (&addr, &c) in self.free_virtual_addresses.iter() {
			if c >= count {
				// Allocate from the bottom half to the top, as lower addresses are usually
				// more readable (i.e. more ergonomic).
				self.free_virtual_addresses.remove(&addr);
				if let Some(new_count) = NonZeroUsize::new(c.get() - count.get()) {
					let addr = addr.as_ptr().wrapping_add(count.get());
					self.free_virtual_addresses
						.insert(NonNull::new(addr).unwrap(), new_count);
				}
				return Ok(addr);
			}
		}
		Err(NoFreeVirtualAddressSpace)
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

#[derive(Clone, Copy)]
pub struct MemoryObjectHandle(usize);

impl From<MemoryObjectHandle> for usize {
	fn from(h: MemoryObjectHandle) -> Self {
		h.0
	}
}

impl From<usize> for MemoryObjectHandle {
	fn from(n: usize) -> Self {
		Self(n)
	}
}

pub struct NoFreeVirtualAddressSpace;
