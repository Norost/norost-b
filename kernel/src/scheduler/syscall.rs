use crate::{driver, object_table};
use crate::object_table::{Id, TableId};
use crate::memory::{frame, Page, r#virtual::RWX};
use crate::scheduler::{process::Process, syscall::frame::DMAFrame};
use crate::scheduler::process::ObjectHandle;
use crate::ffi;
use core::marker::PhantomData;
use core::mem;
use core::ptr::NonNull;
use alloc::{boxed::Box, vec::Vec};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Return {
	pub status: usize,
	pub value: usize,
}

type Syscall = extern "C" fn(usize, usize, usize, usize, usize, usize) -> Return;

pub const SYSCALLS_LEN: usize = 12;
#[export_name = "syscall_table"]
static SYSCALLS: [Syscall; SYSCALLS_LEN] = [
	syslog,
	init_client_queue,
	push_client_queue,
	#[cfg(feature = "driver-pci")]
	driver::pci::syscall::map_any,
	#[cfg(feature = "driver-pci")]
	driver::pci::syscall::map_bar,
	#[cfg(not(feature = "driver-pci"))]
	undefined,
	#[cfg(not(feature = "driver-pci"))]
	undefined,
	alloc_dma,
	physical_address,
	next_table,
	query_table,
	query_next,
	open_object,
	map_object,
];

extern "C" fn syslog(ptr: usize, len: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	// SAFETY: FIXME
	let s = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
	info!(
		"[user log] {}",
		core::str::from_utf8(s).unwrap_or("<illegible>")
	);
	Return {
		status: 0,
		value: len,
	}
}

extern "C" fn init_client_queue(
	address: usize,
	submission_p2size: usize,
	completion_p2size: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	Process::current()
		.init_client_queue(
			address as *mut _,
			submission_p2size as u8,
			completion_p2size as u8,
		)
		.unwrap();
	Return {
		status: 0,
		value: 0,
	}
}

extern "C" fn push_client_queue(
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	Process::current().poll_client_queue().unwrap();
	Return {
		status: 0,
		value: 0,
	}
}

extern "C" fn alloc_dma(base: usize, size: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	let rwx = RWX::RW;
	let base = NonNull::new(base as *mut _);
	let count = (size + Page::MASK) / Page::SIZE;
	let frame = DMAFrame::new(count.try_into().unwrap()).unwrap();
	Process::current().map_memory_object(base, Box::new(frame), rwx).unwrap();
	Return {
		status: 0,
		value: 0,
	}
}

extern "C" fn physical_address(address: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	let address = NonNull::new(address as *mut _).unwrap();
	let value = Process::current().get_physical_address(address).unwrap().0;
	Return {
		status: 0,
		value,
	}
}

#[repr(C)]
struct TableInfo {
	name_len: u8,
	name: [u8; 255],
}

/// Return the name and ID of the table after another table, or the first table if `id == usize::MAX`.
extern "C" fn next_table(id: usize, info_ptr: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	let id = (id != usize::MAX).then(|| TableId::from(u32::try_from(id).unwrap()));
	let (name, id) = match object_table::next_table(id) {
		Some(p) => p,
		None => return Return {
			status: 1,
			value: 0,
		}
	};
	// SAFETY: FIXME
	unsafe {
		let info = &mut *(info_ptr as *mut TableInfo);
		assert!(info.name.len() >= name.len());
		info.name[..name.len()].copy_from_slice(name.as_bytes());
		info.name_len = name.len().try_into().unwrap();
	}
	Return {
		status: 0,
		value: u32::from(id).try_into().unwrap(),
	}
}

extern "C" fn query_table(id: usize, name: usize, name_len: usize, tags: usize, tags_len: usize, _: usize) -> Return {
	let id = TableId::from(u32::try_from(id).unwrap());
	// SAFETY: FIXME
	let (name, tags) = unsafe {
		let name = (name != 0)
			.then(|| core::slice::from_raw_parts(name as *const u8, name_len))
			.map(|s| core::str::from_utf8(s).unwrap());
		let tags = (tags != 0)
			.then(|| core::slice::from_raw_parts(tags as *const ffi::Slice<u8>, tags_len))
			.unwrap_or(&[])
			.iter()
			.map(|f| f.unchecked_as_slice())
			.map(|s| core::str::from_utf8(s).unwrap())
			.collect::<Vec<_>>();
		dbg!(name, tags)
	};
	let query = object_table::query(id, name, &tags).unwrap();
	let handle = Process::current().add_query(query);
	Return {
		status: 0,
		value: handle.into(),
	}
}

#[repr(C)]
struct ObjectInfo {
	id: Id,
	name_len: u8,
	name: [u8; 255],
	tags_len: u8,
	tags_offsets: [u32; 255],
}

extern "C" fn query_next(handle: usize, info: usize, string_buffer: usize, string_buffer_len: usize, _: usize, _: usize) -> Return {
	// SAFETY: FIXME
	let info = unsafe { &mut *(info as *mut ObjectInfo) };
	let string_buffer = unsafe {
		core::slice::from_raw_parts_mut(string_buffer as *mut u8, string_buffer_len)
	};
	match Process::current().get_query_mut(handle.into()).unwrap().next() {
		None => Return {
			status: 1,
			value: 0,
		},
		Some(obj) => {
			info.id = obj.id;
			info.name_len = obj.name.len().try_into().unwrap();
			info.name[..obj.name.len()].copy_from_slice(obj.name.as_bytes());
			info.tags_len = obj.tags.len().try_into().unwrap();
			let mut p = 0;
			for (to, tag) in info.tags_offsets.iter_mut().zip(&*obj.tags) {
				*to = p as u32;
				string_buffer[p] = tag.len().try_into().unwrap();
				string_buffer[p + 1..p + 1 + tag.len()].copy_from_slice(tag.as_bytes());
				p += 1 + tag.len();
			}
			Return {
				status: 0,
				value: 0,
			}
		}
	}
}

extern "C" fn open_object(table_id: usize, id_l: usize, id_h: usize, _: usize, _: usize, _: usize) -> Return {
	let table_id = match mem::size_of_val(&table_id) {
		4 | 8 | 16 => u32::try_from(table_id).unwrap(),
		s => unreachable!("unsupported usize size of {}", s),
	}.into();
	let id = match mem::size_of_val(&id_l) {
		4 => u64::try_from(id_h).unwrap() << 32 | u64::try_from(id_l).unwrap(),
		8 | 16 => id_l.try_into().unwrap(),
		s => unreachable!("unsupported usize size of {}", s),
	}.into();
	let obj = object_table::get(table_id, id).unwrap();
	let handle = Process::current().add_object(obj.unwrap());
	Return {
		status: 0,
		value: handle.unwrap().into(),
	}
}

extern "C" fn map_object(handle: usize, address: usize, _rwx: usize, _: usize, _: usize, _: usize) -> Return {
	let handle = ObjectHandle::from(handle);
	let address = NonNull::new(address as *mut _);
	Process::current().map_memory_object_2(handle, address, RWX::RW);
	Return {
		status: 0,
		value: 0,
	}
}

#[allow(dead_code)]
extern "C" fn undefined(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	Return {
		status: usize::MAX,
		value: 0,
	}
}
