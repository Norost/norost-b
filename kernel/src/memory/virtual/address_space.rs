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
	) -> Result<MemoryObjectHandle, MapError> {

		let base = base.unwrap(); // TODO
		let count = object
			.physical_pages()
			.into_vec()
			.into_iter()
			.flat_map(|f| f)
			.count();
		let end = base.as_ptr().wrapping_add(count).wrapping_sub(1);
		if end < base.as_ptr() {
			Err(MapError::Overflow)?;
		}
		let end = NonNull::new(base.as_ptr()).unwrap();

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
			let h = MemoryObjectHandle(self.objects.len());
			self.objects.push((base..=end, object));
			h
		})
	}

	/// Get a reference to a memory object.
	pub fn get_object(&self, handle: MemoryObjectHandle) -> Option<&dyn MemoryObject> {
		self.objects.get(handle.0).map(|(_, o)| &**o)
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
