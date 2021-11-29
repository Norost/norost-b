use crate::arch::r#virtual;
use crate::memory::{
	r#virtual::{MapError, RWX},
	Page,
};
use crate::scheduler::MemoryObject;
use alloc::{boxed::Box, vec::Vec};
use core::ops::RangeInclusive;
use core::ptr::NonNull;

pub struct AddressSpace {
	/// The address space mapping used by the MMU
	mmu_address_space: r#virtual::AddressSpace,
	/// All mapped objects
	objects: Vec<(RangeInclusive<*const ()>, Box<dyn MemoryObject>)>,
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
	) -> Result<(), MapError> {
		unsafe {
			self.mmu_address_space.map(
				base.map_or(core::ptr::null(), |b| b.as_ptr() as *const _),
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
		}
	}

	/// Map a virtual address to a physical address.
	pub fn get_physical_address(&self, address: NonNull<()>) -> Option<(usize, RWX)> {
		self.mmu_address_space.get_physical_address(address)
	}
}

//TODO temporary because I'm a lazy ass
impl core::ops::Deref for AddressSpace {
	type Target = r#virtual::AddressSpace;
	fn deref(&self) -> &Self::Target {
		&self.mmu_address_space
	}
}
impl core::ops::DerefMut for AddressSpace {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.mmu_address_space
	}
}
