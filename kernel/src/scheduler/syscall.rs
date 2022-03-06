use crate::ffi;
use crate::memory::{frame, r#virtual::RWX, Page};
use crate::object_table;
use crate::object_table::{Id, Job, JobId, JobType, TableId};
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

pub const SYSCALLS_LEN: usize = 18;
#[export_name = "syscall_table"]
static SYSCALLS: [Syscall; SYSCALLS_LEN] = [
	syslog,
	undefined,
	undefined,
	alloc_dma,
	physical_address,
	next_table,
	query_table,
	query_next,
	open_object,
	map_object,
	sleep,
	read_object,
	write_object,
	create_table,
	poll_object,
	take_table_job,
	finish_table_job,
	create_object,
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

extern "C" fn query_table(
	id: usize,
	name: usize,
	name_len: usize,
	tags: usize,
	tags_len: usize,
	_: usize,
) -> Return {
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
		(name, tags)
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
	tags_len: u8,
	tags_offsets: [u32; 255],
}

extern "C" fn query_next(
	handle: usize,
	info: usize,
	string_buffer: usize,
	string_buffer_len: usize,
	_: usize,
	_: usize,
) -> Return {
	// SAFETY: FIXME
	let info = unsafe { &mut *(info as *mut ObjectInfo) };
	let string_buffer =
		unsafe { core::slice::from_raw_parts_mut(string_buffer as *mut u8, string_buffer_len) };
	match Process::current()
		.get_query_mut(handle.into())
		.unwrap()
		.next()
	{
		None => Return {
			status: 1,
			value: 0,
		},
		Some(obj) => {
			info.id = obj.id;
			info.tags_len = obj.tags.len().try_into().unwrap();
			let mut p = 0;
			for (to, tag) in info.tags_offsets.iter_mut().zip(&*obj.tags) {
				*to = p as u32;
				let q = p + 1 + tag.len();
				if q >= string_buffer.len() {
					// There is not enough space to copy the tag, so just skip it and
					// the remaining tags.
					break;
				}
				string_buffer[p] = tag.len().try_into().unwrap();
				string_buffer[p + 1..q].copy_from_slice(tag.as_bytes());
				p = q;
			}
			Return {
				status: 0,
				value: 0,
			}
		}
	}
}

extern "C" fn create_object(
	table_id: usize,
	tags_ptr: usize,
	tags_len: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	let table_id = TableId(table_id as u32);
	let tags = unsafe { core::slice::from_raw_parts(tags_ptr as *const u8, tags_len) };
	let ticket = object_table::create(table_id, tags).unwrap();
	let obj = super::block_on(ticket).unwrap().into_object().unwrap();
	let handle = Process::current().add_object(obj);
	Return {
		status: 0,
		value: handle.unwrap().into(),
	}
}

extern "C" fn open_object(
	table_id: usize,
	id_l: usize,
	id_h: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	let table_id = match mem::size_of_val(&table_id) {
		4 | 8 | 16 => u32::try_from(table_id).unwrap(),
		s => unreachable!("unsupported usize size of {}", s),
	}
	.into();
	let id = Id::from(merge_u64(id_l, id_h));
	let ticket = object_table::get(table_id, id).unwrap();
	let obj = super::block_on(ticket).unwrap().into_object().unwrap();
	let handle = Process::current().add_object(obj);
	Return {
		status: 0,
		value: handle.unwrap().into(),
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

extern "C" fn read_object(
	handle: usize,
	base: usize,
	length: usize,
	offset_l: usize,
	offset_h: usize,
	_: usize,
) -> Return {
	let handle = ObjectHandle::from(handle);
	let offset = merge_u64(offset_l, offset_h);
	let base = NonNull::new(base as *mut u8).unwrap();
	let data = unsafe { core::slice::from_raw_parts_mut(base.as_ptr(), length) };

	let read = Process::current()
		.get_object(handle)
		.unwrap()
		.read(offset, data)
		.unwrap();
	let read = super::block_on(read).unwrap().into_usize().unwrap();

	Return {
		status: 0,
		value: read,
	}
}

extern "C" fn write_object(
	handle: usize,
	base: usize,
	length: usize,
	offset_l: usize,
	offset_h: usize,
	_: usize,
) -> Return {
	let handle = ObjectHandle::from(handle);
	let offset = merge_u64(offset_l, offset_h);
	let base = NonNull::new(base as *mut u8).unwrap();
	let data = unsafe { core::slice::from_raw_parts(base.as_ptr(), length) };

	let written = Process::current()
		.get_object(handle)
		.unwrap()
		.write(offset, data)
		.unwrap();
	let written = super::block_on(written).unwrap().into_usize().unwrap();

	Return {
		status: 0,
		value: written,
	}
}

extern "C" fn poll_object(
	_handle: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	todo!();
	/*
	let handle = ObjectHandle::from(handle);
	let object = Process::current().get_object(handle).unwrap();
	let event = super::block_on(object.event_listener().unwrap());
	Return {
		status: 0,
		value: u32::from(event).try_into().unwrap(),
	}
	*/
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
	pub object_id: Id,
	pub buffer: Option<NonNull<u8>>,
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
				fj.buffer.unwrap().as_ptr(),
				fj.buffer_size.try_into().unwrap(),
			)
		};
		Ok(Self {
			ty: fj.ty.try_into()?,
			flags: fj.flags,
			job_id: fj.job_id,
			object_id: fj.object_id,
			operation_size: fj.operation_size,
			buffer: buffer.into(),
		})
	}
}

extern "C" fn take_table_job(
	handle: usize,
	job_ptr: usize,
	timeout_micros: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	assert_ne!(job_ptr, 0);

	let handle = ObjectHandle::from(handle);
	let tbl = Process::current()
		.get_object(handle)
		.unwrap()
		.clone()
		.as_table()
		.unwrap();

	let mut job = unsafe { &mut *(job_ptr as *mut FfiJob) };
	let copy_to = unsafe {
		core::slice::from_raw_parts_mut(
			job.buffer.unwrap().as_ptr(),
			job.buffer_size.try_into().unwrap(),
		)
	};
	let timeout = Duration::from_micros(timeout_micros.try_into().unwrap());
	let Ok(Ok(info)) = super::block_on_timeout(tbl.take_job(timeout), timeout) else {
		return Return { status: 1, value: 0 };
	};
	job.ty = info.ty.into();
	job.flags = info.flags;
	job.job_id = info.job_id;
	job.object_id = info.object_id;
	job.operation_size = info.operation_size;
	let size = usize::try_from(info.operation_size).unwrap();
	assert!(copy_to.len() >= size, "todo");
	copy_to[..size].copy_from_slice(&info.buffer[..size]);

	Return {
		status: 0,
		value: 0,
	}
}

extern "C" fn finish_table_job(
	handle: usize,
	job_ptr: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	assert_ne!(job_ptr, 0);

	let handle = ObjectHandle::from(handle);
	let tbl = Process::current()
		.get_object(handle)
		.unwrap()
		.clone()
		.as_table()
		.unwrap();

	let data = unsafe { (job_ptr as *mut FfiJob).read() };

	tbl.finish_job(data.try_into().unwrap()).unwrap();

	Return {
		status: 0,
		value: 0,
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
