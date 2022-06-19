use crate::Handle;
use core::{ops::RangeInclusive, ptr::NonNull};

#[derive(Clone, Copy)]
pub enum NewObjectType {
	MemoryMap = 0,
	Root = 1,
	Duplicate = 2,
}

impl NewObjectType {
	pub fn into_raw(self) -> u8 {
		self as _
	}

	pub fn try_from_raw(n: impl TryInto<u8>) -> Option<Self> {
		n.try_into().ok().and_then(|n| {
			Some(match n {
				0 => Self::MemoryMap,
				1 => Self::Root,
				2 => Self::Duplicate,
				_ => return None,
			})
		})
	}
}

pub enum NewObject {
	MemoryMap { range: RangeInclusive<NonNull<u8>> },
	Root,
	Duplicate { handle: Handle },
}
