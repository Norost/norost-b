use crate::time::Monotonic;
use crate::{
	arch,
	memory::{
		frame,
		frame::OwnedPageFrames,
		r#virtual::{self, AddressSpace, MapError, RWX},
		Page,
	},
	object_table::{
		Handle, NewStreamingTableError, Object, Pipe, Root, SeekFrom, StreamingTable, SubRange,
		TinySlice,
	},
	scheduler::{self, process::Process, Thread},
	util::{erase_handle, unerase_handle},
};
use alloc::{boxed::Box, sync::Arc};
use core::mem;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::time::Duration;
use norostb_kernel::{error::Error, io::Request, object::NewObject};

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Return {
	pub status: usize,
	pub value: usize,
}

impl Return {
	const INVALID_OBJECT: Self = Self::error(Error::InvalidObject);
	const INVALID_OPERATION: Self = Self::error(Error::InvalidObject);
	const INVALID_DATA: Self = Self::error(Error::InvalidData);

	const fn error(error: Error) -> Self {
		Self {
			status: error as _,
			value: 0,
		}
	}

	const fn handle(handle: Handle) -> Self {
		Self {
			status: 0,
			value: handle as _,
		}
	}
}

type Syscall = extern "C" fn(usize, usize, usize, usize, usize, usize) -> Return;

pub const SYSCALLS_LEN: usize = 15;

/// Helper type to ensure the syscall table is aligned to a cache boundary, which
/// improves efficiency when using the first 8 syscalls (which all fit inside a single
/// cache line).
#[repr(align(64))]
#[repr(C)]
struct SyscallTable([Syscall; SYSCALLS_LEN]);

#[export_name = "syscall_table"]
static SYSCALLS: SyscallTable = SyscallTable([
	alloc,
	dealloc,
	new_object,
	map_object,
	do_io,
	poll_io_queue,
	wait_io_queue,
	monotonic_time,
	sleep,
	exit,
	spawn_thread,
	wait_thread,
	exit_thread,
	create_io_queue,
	destroy_io_queue,
]);

fn raw_to_rwx(rwx: usize) -> Option<RWX> {
	Some(match rwx {
		0b100 => RWX::R,
		0b010 => RWX::W,
		0b001 => RWX::X,
		0b110 => RWX::RW,
		0b101 => RWX::RX,
		0b111 => RWX::RWX,
		_ => return None,
	})
}

extern "C" fn alloc(base: usize, size: usize, rwx: usize, _: usize, _: usize, _: usize) -> Return {
	debug!("alloc {:#x} {} {:#03b}", base, size, rwx);
	let Some(count) = NonZeroUsize::new((size + Page::MASK) / Page::SIZE) else {
		return Return {
			status: Error::InvalidData as _,
			value: 0,
		};
	};
	let Some(rwx) = raw_to_rwx(rwx) else {
		return Return {
			status: Error::InvalidData as _,
			value: 0,
		};
	};
	let proc = Process::current().unwrap();
	let base = base as *mut _;
	match OwnedPageFrames::new(count, proc.allocate_hints(base)) {
		Ok(mut mem) => {
			unsafe { mem.clear() };
			proc.map_memory_object(NonNull::new(base.cast()), Box::new(mem), rwx)
				.map_or(
					Return {
						status: Error::Unknown as _,
						value: 0,
					},
					|base| Return {
						status: count.get() * Page::SIZE,
						value: base.as_ptr() as usize,
					},
				)
		}
		Err(_) => Return {
			status: Error::CantCreateObject as _,
			value: 0,
		},
	}
}

extern "C" fn dealloc(base: usize, size: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	debug!("dealloc {:#x} {}", base, size);
	if base & Page::MASK != 0 || size & Page::MASK != 0 {
		return Return {
			status: Error::InvalidData as _,
			value: 0,
		};
	}
	let Some(base) = NonNull::new(base as *mut _) else {
		return Return {
			status: 0,
			value: 0,
		};
	};
	let Some(count) = NonZeroUsize::new(size / Page::MASK) else {
		return Return {
			status: 0,
			value: 0,
		};
	};
	Process::current()
		.unwrap()
		.unmap_memory_object(base, count)
		.map_or(
			Return {
				status: Error::Unknown as _,
				value: 0,
			},
			|_| Return {
				status: size,
				value: base.as_ptr() as usize,
			},
		)
}

extern "C" fn monotonic_time(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	get_mono_time()
}

// Limit to 64 bit for now since we can't pass enough data in registers on e.g. x86
#[cfg(target_pointer_width = "64")]
extern "C" fn do_io(ty: usize, handle: usize, a: usize, b: usize, c: usize, _: usize) -> Return {
	use super::block_on;
	let handle = unerase_handle(handle as _);
	debug!("do_io {} {:?} {:#x} {:#x} {:#x}", ty, handle, a, b, c);
	Process::current().unwrap().objects_operate(|objects| {
		let Some(o) = objects.get(handle) else { return Return::INVALID_OBJECT };
		let Ok(ty) = ty.try_into() else { return Return::INVALID_OPERATION };
		let return_u64 = |r| Return {
			status: 0,
			value: r as _,
		};
		let ins = |l: &mut arena::Arena<_, _>, o| Return::handle(erase_handle(l.insert(o)));
		match ty {
			Request::READ | Request::PEEK => block_on(o.clone().read(b, ty == Request::PEEK))
				.map_or_else(Return::error, |r| {
					assert!(r.len() <= b, "object returned too much data");
					unsafe { (a as *mut u8).copy_from_nonoverlapping(r.as_ptr(), r.len()) }
					Return {
						status: 0,
						value: r.len(),
					}
				}),
			Request::WRITE => {
				let r = unsafe { core::slice::from_raw_parts(a as *const u8, b) };
				block_on(o.clone().write(r)).map_or_else(Return::error, |r| Return {
					status: 0,
					value: r.try_into().unwrap(),
				})
			}
			Request::GET_META => {
				let (prop_len, value_len) = (c as u8, (c >> 8) as u8);
				let prop = unsafe { TinySlice::from_raw_parts(a as *const u8, prop_len) };
				block_on(o.clone().get_meta(prop)).map_or_else(Return::error, |r| {
					let l = usize::from(value_len).min(r.len());
					unsafe { (b as *mut u8).copy_from_nonoverlapping(r.as_ptr(), l) }
					Return {
						status: 0,
						value: l.try_into().unwrap(),
					}
				})
			}
			Request::SET_META => {
				let (prop_len, value_len) = (c as u8, (c >> 8) as u8);
				let prop = unsafe { TinySlice::from_raw_parts(a as *const u8, prop_len) };
				let value = unsafe { TinySlice::from_raw_parts(b as *const u8, value_len) };
				block_on(o.clone().set_meta(prop, value)).map_or_else(Return::error, |r| Return {
					status: 0,
					value: {
						debug_assert_eq!(r & !1, 0, "meta result is not boolean");
						r.try_into().unwrap()
					},
				})
			}
			Request::OPEN | Request::CREATE => {
				let r = unsafe { core::slice::from_raw_parts(a as *const u8, b) };
				let o = o.clone();
				block_on(if ty == Request::OPEN {
					o.open(r)
				} else {
					o.create(r)
				})
				.map_or_else(Return::error, |o| ins(objects, o))
			}
			Request::DESTROY => {
				let r = unsafe { core::slice::from_raw_parts(a as *const u8, b) };
				block_on(o.clone().destroy(r)).map_or_else(Return::error, return_u64)
			}
			Request::SEEK => a
				.try_into()
				.ok()
				.and_then(|a| SeekFrom::try_from_raw(a, b as _).ok())
				.map_or(Return::INVALID_DATA, |s| {
					block_on(o.seek(s)).map_or_else(Return::error, return_u64)
				}),
			Request::CLOSE => Return {
				status: objects
					.remove(handle)
					.map_or(Error::InvalidObject as _, |_| 0),
				value: 0,
			},
			Request::SHARE => objects
				.get(unerase_handle(a as _))
				.map_or(Return::INVALID_OBJECT, |s| {
					block_on(o.share(s)).map_or_else(Return::error, return_u64)
				}),
			_ => Return::INVALID_OPERATION,
		}
	})
}

extern "C" fn new_object(ty: usize, a: usize, b: usize, c: usize, _: usize, _: usize) -> Return {
	debug!("new_object {} {:#x} {:#x} {:#x}", ty, a, b, c);
	let Some(args) = NewObject::try_from_args(ty, a, b, c) else {
		return Return {
			status: Error::InvalidData as _,
			value: 0,
		}
	};
	let proc = Process::current().unwrap();
	let hints = proc.allocate_hints(0 as _);
	match args {
		NewObject::SubRange { handle, range } => proc
			.object_transform_new(handle, |o| {
				o.clone()
					.memory_object()
					.ok_or(Error::InvalidOperation)
					.and_then(|o| SubRange::new(o.clone(), range).map_err(|_| Error::InvalidData))
					.map(|o| o as Arc<dyn Object>)
			})
			.ok_or(Error::InvalidObject)
			.flatten(),
		NewObject::Root => proc
			.add_object(Arc::new(Root::new()))
			.map_err(|e| match e {}),
		NewObject::Duplicate { handle } => proc
			.duplicate_object_handle(handle)
			.ok_or(Error::InvalidObject),
		NewObject::SharedMemory { size } => NonZeroUsize::new((size + Page::MASK) / Page::SIZE)
			.ok_or(Error::InvalidData)
			.and_then(|s| {
				OwnedPageFrames::new(s, proc.allocate_hints(0 as _)).map_err(|e| match e {
					frame::AllocateError::OutOfFrames => Error::CantCreateObject,
				})
			})
			.map(|o| Arc::new(o) as Arc<dyn Object>)
			.and_then(|o| proc.add_object(o).map_err(|e| match e {})),
		NewObject::StreamTable {
			buffer_mem,
			buffer_mem_block_size,
			allow_sharing,
			max_request_mem,
		} => proc
			.object_transform_new(buffer_mem, |buffer_mem| {
				if let Some(buffer_mem) = buffer_mem.clone().memory_object() {
					StreamingTable::new(
						allow_sharing,
						buffer_mem,
						buffer_mem_block_size,
						max_request_mem,
						hints,
					)
					.map_err(|e| match e {
						NewStreamingTableError::Alloc(_) => Error::CantCreateObject,
						NewStreamingTableError::Map(_) => Error::CantCreateObject,
						NewStreamingTableError::BlockSizeTooLarge => Error::InvalidData,
					})
					.map(|o| o as Arc<dyn Object>)
				} else {
					Err(Error::InvalidData)
				}
			})
			.unwrap_or(Err(Error::InvalidObject)),
		NewObject::PermissionMask { handle, rwx } => proc
			.object_transform_new(handle, |o| {
				r#virtual::mask_permissions_object(o.clone(), rwx).ok_or(Error::InvalidData)
			})
			.unwrap_or(Err(Error::InvalidObject)),
		NewObject::Pipe => proc.add_object(Pipe::new()).map_err(|e| match e {}),
	}
	.map_or_else(
		|e| Return {
			status: e as _,
			value: 0,
		},
		|h| Return {
			status: 0,
			value: h.try_into().unwrap(),
		},
	)
}

extern "C" fn map_object(
	handle: usize,
	base: usize,
	rwx: usize,
	offset: usize,
	max_length: usize,
	_: usize,
) -> Return {
	debug!(
		"map_object {:?} {:#x} {:03b} {} {}",
		unerase_handle(handle as _),
		base,
		rwx,
		offset,
		max_length
	);
	let Ok(rwx) = RWX::from_flags(rwx & 4 != 0, rwx & 2 != 0, rwx & 1 != 0) else {
		return Return::INVALID_DATA;
	};
	Process::current()
		.unwrap()
		.map_memory_object_2(
			handle as Handle,
			NonNull::new(base as _),
			rwx,
			offset,
			max_length,
		)
		.map_or_else(
			|e| Return {
				status: (match e {
					MapError::Overflow
					| MapError::ZeroSize
					| MapError::Permission
					| MapError::UnalignedOffset => Error::InvalidData,
					MapError::Arch(e) => todo!("{:?}", e),
				}) as _,
				value: 0,
			},
			|(base, length)| Return {
				status: length,
				value: base.as_ptr() as usize,
			},
		)
}

extern "C" fn sleep(
	time_l: usize,
	time_h: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("sleep");
	let time = merge_u64(time_l, time_h);
	let time = Duration::from_nanos(time.into());
	Thread::current().unwrap().sleep(time);
	get_mono_time()
}

extern "C" fn spawn_thread(
	start: usize,
	stack: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("spawn_thread");
	Process::current()
		.unwrap()
		.spawn_thread(start, stack)
		.map_or(
			Return {
				status: 1,
				value: 0,
			},
			|handle| Return {
				status: 0,
				value: handle.try_into().unwrap(),
			},
		)
}

extern "C" fn create_io_queue(
	base: usize,
	request_p2size: usize,
	response_p2size: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("create_io_queue");
	Process::current()
		.unwrap()
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

extern "C" fn destroy_io_queue(
	base: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("destroy_io_queue {:#x}", base);
	NonNull::new(base as *mut _).map_or(
		Return {
			status: 1,
			value: 0,
		},
		|base| {
			Process::current()
				.unwrap()
				.destroy_io_queue(base)
				.map_or_else(
					|_| Return {
						status: 1,
						value: 0,
					},
					|()| Return {
						status: 0,
						value: 0,
					},
				)
		},
	)
}

extern "C" fn poll_io_queue(
	base: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("poll_io_queue {:#x}", base);
	let Some(base) = NonNull::new(base as *mut _) else {
		return Return { status: Error::InvalidData as usize, value: 0 }
	};
	Process::current().unwrap().process_io_queue(base).map_or(
		Return {
			status: Error::Unknown as usize,
			value: 0,
		},
		|_| get_mono_time(),
	)
}

extern "C" fn wait_io_queue(
	base: usize,
	timeout_l: usize,
	timeout_h: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("wait_io_queue");
	let Some(base) = NonNull::new(base as *mut _) else {
		return Return { status: Error::InvalidData as usize, value: 0 }
	};
	let timeout = merge_u64(timeout_l, timeout_h);
	let timeout = Duration::from_nanos(timeout.into());
	Process::current()
		.unwrap()
		.wait_io_queue(base, timeout)
		.map_or(
			Return {
				status: Error::Unknown as usize,
				value: 0,
			},
			|_| get_mono_time(),
		)
}

extern "C" fn exit_thread(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	debug!("exit_thread");
	let thread = Arc::into_raw(Thread::current().unwrap());
	arch::run_on_local_cpu_stack_noreturn!(destroy_thread, thread.cast());

	extern "C" fn destroy_thread(data: *const ()) -> ! {
		let thread = unsafe { Arc::from_raw(data.cast::<Thread>()) };

		arch::amd64::clear_current_thread();

		unsafe {
			AddressSpace::activate_default();
		}

		// SAFETY: we switched to the CPU local stack and won't return to the stack of this thread
		// We also switched to the default address space in case it's the last thread of the
		// process.
		unsafe {
			thread.destroy();
		}

		// SAFETY: there is no thread state to save.
		unsafe { scheduler::next_thread() }
	}
}

extern "C" fn wait_thread(
	handle: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("wait_thread");
	Process::current()
		.unwrap()
		.get_thread(handle as u32)
		.map_or(
			Return {
				status: usize::MAX,
				value: 0,
			},
			|thread| {
				thread.wait().unwrap();
				Return {
					status: 0,
					value: 0,
				}
			},
		)
}

extern "C" fn exit(code: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	debug!("exit");
	#[derive(Clone, Copy)]
	struct D(*const Process, u8);
	let proc = Process::current().unwrap();
	proc.prepare_destroy();
	let d = D(Arc::into_raw(proc), code as _);
	arch::run_on_local_cpu_stack_noreturn!(destroy_process, &d as *const _ as _);

	extern "C" fn destroy_process(data: *const ()) -> ! {
		let D(process, code) = unsafe { data.cast::<D>().read() };
		let process = unsafe { Arc::from_raw(process) };

		arch::amd64::clear_current_thread();

		unsafe {
			AddressSpace::activate_default();
		}

		// SAFETY: we switched to the CPU local stack and won't return to a stack of a thread
		// owned by this process. We also switched to the default address space.
		unsafe {
			process.destroy(code);
		}

		// SAFETY: there is no thread state to save.
		unsafe { scheduler::next_thread() }
	}
}

fn merge_u64(l: usize, h: usize) -> u64 {
	match mem::size_of_val(&l) {
		4 => (h as u64) << 32 | l as u64,
		8 | 16 => l as u64,
		s => unreachable!("unsupported usize size of {}", s),
	}
}

fn get_mono_time() -> Return {
	let now = Monotonic::now().as_nanos();
	#[cfg(target_pointer_width = "32")]
	return Return {
		status: (now >> 32) as usize,
		value: now as usize,
	};
	#[cfg(target_pointer_width = "64")]
	return Return {
		status: 0,
		value: now as usize,
	};
}
