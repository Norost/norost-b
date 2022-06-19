use super::ControlHeader;
use endian::u32le;
use virtio::PhysAddr;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct AttachBacking {
	header: ControlHeader,
	pub(crate) resource_id: u32le,
	pub(crate) entities_count: u32le,
}

impl AttachBacking {
	pub fn new(resource_id: u32, count: u32, fence: Option<u64>) -> Self {
		Self {
			header: ControlHeader::new(ControlHeader::CMD_RESOURCE_ATTACH_BACKING, fence),
			resource_id: resource_id.into(),
			entities_count: count.into(),
		}
	}
}

#[derive(Clone, Copy)] // Mainly so we can use it with arrays.
#[repr(C)]
pub struct MemoryEntry {
	address: PhysAddr,
	length: u32le,
	_padding: u32le,
}

impl MemoryEntry {
	pub fn new(address: PhysAddr, length: u32) -> Self {
		Self {
			address,
			length: length.into(),
			_padding: 0.into(),
		}
	}
}
