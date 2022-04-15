use crate::ffi;
use crate::memory::{frame, r#virtual::RWX, Page};
use crate::object_table;
use crate::object_table::{Handle, Job, JobId, JobType, QueryId, TableId};
use crate::scheduler::process::ObjectHandle;
use crate::scheduler::{process::Process, syscall::frame::DMAFrame, Thread};
use crate::time::Monotonic;
use alloc::{
	boxed::Box,
	sync::{Arc, Weak},
	vec::Vec,
};
use core::arch::asm;
use core::mem;
use core::ptr::NonNull;
use core::time::Duration;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Return {
	pub status: usize,
	pub value: usize,
}

type Syscall = extern "C" fn(usize, usize, usize, usize, usize, usize) -> Return;

pub const SYSCALLS_LEN: usize = 23;
#[export_name = "syscall_table"]
static SYSCALLS: [Syscall; SYSCALLS_LEN] = [
	undefined,
	undefined,
	undefined,
	alloc_dma,
	physical_address,
	next_table,
	undefined,
	undefined,
	undefined,
	map_object,
	sleep,
	undefined,
	undefined,
	create_table,
	undefined,
	undefined,
	undefined,
	undefined,
	duplicate_handle,
	spawn_thread,
	create_io_rings,
	submit_io,
	wait_io,
];

extern "C" fn alloc_dma(
	base: usize,
	size: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	let rwx = RWX::RW;
	let base = NonNull::new(base as *mut _);
	let count = (size + Page::MASK) / Page::SIZE;
	let frame = DMAFrame::new(count.try_into().unwrap()).unwrap();
	Process::current()
		.map_memory_object(base, Box::new(frame), rwx)
		.unwrap();
	Return {
		status: 0,
		value: count * Page::SIZE,
	}
}

extern "C" fn physical_address(
	address: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	let address = NonNull::new(address as *mut _).unwrap();
	let value = Process::current().get_physical_address(address).unwrap().0;
	Return { status: 0, value }
}

#[repr(C)]
struct TableInfo {
	name_len: u8,
	name: [u8; 255],
}

/// Return the name and ID of the table after another table, or the first table if `id == usize::MAX`.
extern "C" fn next_table(
	id: usize,
	info_ptr: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	let id = (id != usize::MAX).then(|| TableId::from(u32::try_from(id).unwrap()));
	let (name, id) = match object_table::next_table(id) {
		Some(p) => p,
		None => {
			return Return {
				status: 1,
				value: 0,
			}
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

extern "C" fn map_object(
	handle: usize,
	base: usize,
	offset_l: usize,
	offset_h_or_length: usize,
	length_or_rwx: usize,
	rwx: usize,
) -> Return {
	let (offset, _length, _rwx) = match mem::size_of_val(&offset_l) {
		4 => (
			(offset_h_or_length as u64) << 32 | offset_l as u64,
			length_or_rwx,
			rwx,
		),
		8 | 16 => (offset_l as u64, offset_h_or_length, length_or_rwx),
		s => unreachable!("unsupported usize size of {}", s),
	};
	let handle = ObjectHandle::from(handle);
	let base = NonNull::new(base as *mut _);
	Process::current()
		.map_memory_object_2(handle, base, offset, RWX::RW)
		.unwrap();
	Return {
		status: 0,
		value: base.unwrap().as_ptr() as usize,
	}
}

extern "C" fn duplicate_handle(
	handle: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	let handle = ObjectHandle::from(handle);

	Process::current().duplicate_object_handle(handle).map_or(
		Return {
			status: 1,
			value: 0,
		},
		|handle| Return {
			status: 0,
			value: handle.into(),
		},
	)
}

extern "C" fn create_table(
	name: usize,
	name_len: usize,
	ty: usize,
	_options: usize,
	_: usize,
	_: usize,
) -> Return {
	let name = NonNull::new(name as *mut u8).unwrap();
	assert!(name_len <= 255, "name too long");
	let name = unsafe { core::slice::from_raw_parts(name.as_ptr(), name_len) };
	let name = core::str::from_utf8(name).unwrap();

	let name = name.into();
	let tbl = match ty {
		0 => {
			let tbl = object_table::StreamingTable::new(name);
			object_table::add_table(Arc::downgrade(&tbl) as Weak<dyn object_table::Table>);
			tbl
		}
		_ => todo!(),
	};

	let handle = Process::current().add_object(tbl).unwrap();

	Return {
		status: 0,
		value: handle.into(),
	}
}

#[derive(Default, Debug)]
#[repr(C)]
pub struct FfiJob {
	pub ty: FfiJobType,
	pub flags: [u8; 3],
	pub job_id: JobId,
	pub buffer_size: u32,
	pub operation_size: u32,
	pub handle: Handle,
	pub buffer: Option<NonNull<u8>>,
	pub query_id: QueryId,
	pub from_anchor: u8,
	pub from_offset: u64,
}

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct FfiJobType(u8);

default!(newtype FfiJobType = u8::MAX);

impl From<JobType> for FfiJobType {
	fn from(jt: JobType) -> Self {
		FfiJobType(jt as u8)
	}
}

#[derive(Debug)]
pub struct UnknownJobType;

impl TryFrom<FfiJobType> for JobType {
	type Error = UnknownJobType;

	fn try_from(fjt: FfiJobType) -> Result<Self, Self::Error> {
		match fjt.0 {
			0 => Ok(Self::Open),
			1 => Ok(Self::Read),
			2 => Ok(Self::Write),
			3 => Ok(Self::Query),
			4 => Ok(Self::Create),
			5 => Ok(Self::QueryNext),
			6 => Ok(Self::Seek),
			_ => Err(UnknownJobType),
		}
	}
}

impl TryFrom<FfiJob> for Job {
	type Error = UnknownJobType;

	fn try_from(fj: FfiJob) -> Result<Self, Self::Error> {
		// TODO don't panic
		let buffer = unsafe {
			core::slice::from_raw_parts(
				fj.buffer
					.map_or(mem::align_of::<u8>() as *const _, |p| p.as_ptr()),
				fj.buffer_size.try_into().unwrap(),
			)
		};
		Ok(Self {
			ty: fj.ty.try_into()?,
			flags: fj.flags,
			job_id: fj.job_id,
			handle: fj.handle,
			operation_size: fj.operation_size,
			buffer: buffer.into(),
			query_id: fj.query_id,
			from_anchor: fj.from_anchor,
			from_offset: fj.from_offset,
		})
	}
}

extern "C" fn sleep(
	time_l: usize,
	time_h: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	let time = merge_u64(time_l, time_h);
	let time = Duration::from_micros(time.into());

	Thread::current().set_sleep_until(Monotonic::now().saturating_add(time));
	Thread::yield_current();

	Return {
		status: 0,
		value: 0,
	}
}

extern "C" fn spawn_thread(
	start: usize,
	stack: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	Process::current().spawn_thread(start, stack).map_or(
		Return {
			status: 1,
			value: 0,
		},
		|handle| Return {
			status: 0,
			value: handle,
		},
	)
}

extern "C" fn create_io_rings(
	base: usize,
	request_p2size: usize,
	response_p2size: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	Process::current()
		.create_io_queue(
			NonNull::new(base as *mut _),
			request_p2size as u8,
			response_p2size as u8,
		)
		.map_or_else(
			|_| Return {
				status: 1,
				value: 0,
			},
			|base| Return {
				status: 0,
				value: base.as_ptr() as usize,
			},
		)
}

extern "C" fn submit_io(base: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	let Some(base) = NonNull::new(base as *mut _) else { return Return { status: 1, value: 0 } };
	Process::current().process_io_queue(base).map_or(
		Return {
			status: 1,
			value: 0,
		},
		|_| Return {
			status: 0,
			value: 0,
		},
	)
}

extern "C" fn wait_io(base: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	let Some(base) = NonNull::new(base as *mut _) else { return Return { status: 1, value: 0 } };
	Process::current().wait_io_queue(base).map_or(
		Return {
			status: 1,
			value: 0,
		},
		|_| Return {
			status: 0,
			value: 0,
		},
	)
}

#[allow(dead_code)]
extern "C" fn undefined(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	Return {
		status: usize::MAX,
		value: 0,
	}
}

fn merge_u64(l: usize, h: usize) -> u64 {
	match mem::size_of_val(&l) {
		4 => (h as u64) << 32 | l as u64,
		8 | 16 => l as u64,
		s => unreachable!("unsupported usize size of {}", s),
	}
}
