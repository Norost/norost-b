//! A `PermissionMask` object restricts the permissions that can be used when mapping an object.

use super::{PPN, RWX};
use crate::object_table::{MemoryObject, Object};
use alloc::sync::Arc;

macro_rules! pm {
	($name:ident $perm:ident) => {
		struct $name(Arc<dyn MemoryObject>);

		unsafe impl MemoryObject for $name {
			fn physical_pages(&self, f: &mut dyn FnMut(&[PPN]) -> bool) {
				self.0.physical_pages(f)
			}

			fn physical_pages_len(&self) -> usize {
				self.0.physical_pages_len()
			}

			fn page_permissions(&self) -> RWX {
				RWX::$perm
			}
		}

		impl Object for $name {
			fn memory_object(self: Arc<Self>) -> Option<Arc<dyn MemoryObject>> {
				Some(self)
			}
		}
	};
}

pm!(PermissionMaskR R);
pm!(PermissionMaskW W);
pm!(PermissionMaskX X);
pm!(PermissionMaskRW RW);
pm!(PermissionMaskRX RX);

pub fn mask_permissions_object(obj: Arc<dyn Object>, rwx: RWX) -> Option<Arc<dyn Object>> {
	if rwx == RWX::RWX {
		return Some(obj);
	}
	let o = obj.clone().memory_object()?;
	let perm = o.page_permissions();
	if perm.is_subset_of(rwx) {
		return Some(obj);
	}
	Some(match perm.intersection(rwx)? {
		RWX::R => Arc::new(PermissionMaskR(o)),
		RWX::W => Arc::new(PermissionMaskW(o)),
		RWX::X => Arc::new(PermissionMaskX(o)),
		RWX::RW => Arc::new(PermissionMaskRW(o)),
		RWX::RX => Arc::new(PermissionMaskRX(o)),
		RWX::RWX => unreachable!(),
	})
}
