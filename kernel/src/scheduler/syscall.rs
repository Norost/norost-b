use crate::memory::{
	frame,
	frame::OwnedPageFrames,
	r#virtual::{AddressSpace, RWX},
	Page,
};
use crate::scheduler;
use crate::scheduler::{process::Process, syscall::frame::DMAFrame, Thread};
use crate::time::Monotonic;
use alloc::{boxed::Box, sync::Arc};
use core::mem;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::time::Duration;
use norostb_kernel::error::Error;

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
	alloc,
	dealloc,
	monotonic_time,
	alloc_dma,
	physical_address,
	undefined,
	undefined,
	undefined,
	undefined,
	map_object,
	sleep,
	undefined,
	undefined,
	destroy_io_queue,
	kill_thread,
	wait_thread,
	exit,
	create_root,
	duplicate_handle,
	spawn_thread,
	create_io_rings,
	submit_io,
	wait_io,
];

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
	debug!("alloc {:#x} {} {:#b}", base, size, rwx);
	let Some(count) = NonZeroUsize::new((size + Page::MASK) / Page::SIZE) else {
		return Return {
			status: 1,
			value: 0,
		};
	};
	let Some(rwx) = raw_to_rwx(rwx) else {
		return Return {
			status: 1,
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
						status: usize::MAX,
						value: 0,
					},
					|base| Return {
						status: count.get() * Page::SIZE,
						value: base.as_ptr() as usize,
					},
				)
		}
		Err(_) => Return {
			status: usize::MAX - 1,
			value: 0,
		},
	}
}

extern "C" fn dealloc(
	base: usize,
	size: usize,
	flags: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("dealloc");
	let dealloc_partial_start = flags & 1 > 0;
	let dealloc_partial_end = flags & 2 > 0;

	// Round up base & size depending on flags.
	let (base, size) = if dealloc_partial_start {
		(base & !Page::MASK, (size + base) & Page::MASK)
	} else {
		(
			(base + Page::MASK) & !Page::MASK,
			size - (Page::SIZE.wrapping_sub(base) & Page::MASK),
		)
	};

	let (base, size) = if dealloc_partial_end {
		(base, (size + Page::MASK) & !Page::MASK)
	} else {
		(base, size & !Page::MASK)
	};

	let Some(count) = NonZeroUsize::new(size / Page::MASK) else {
		return Return {
			status: 0,
			value: 0,
		};
	};
	let Some(base) = NonNull::new(base as *mut _) else {
		return Return {
			status: usize::MAX,
			value: 0,
		}
	};
	Process::current()
		.unwrap()
		.unmap_memory_object(base, count)
		.map_or(
			Return {
				status: usize::MAX - 1,
				value: 0,
			},
			|_| Return {
				status: 0,
				value: size,
			},
		)
}

extern "C" fn monotonic_time(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	get_mono_time()
}

extern "C" fn alloc_dma(
	base: usize,
	size: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("alloc_dma");
	let rwx = RWX::RW;
	let base = NonNull::new(base as *mut _);
	let count = (size + Page::MASK) / Page::SIZE;
	let frame = DMAFrame::new(count.try_into().unwrap()).unwrap();
	Process::current()
		.unwrap()
		.map_memory_object(base, Box::new(frame), rwx)
		.map_or(
			Return {
				status: usize::MAX,
				value: 0,
			},
			|base| Return {
				status: count * Page::SIZE,
				value: base.as_ptr() as usize,
			},
		)
}

extern "C" fn physical_address(
	address: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("physical_address");
	let address = NonNull::new(address as *mut _).unwrap();
	let value = Process::current()
		.unwrap()
		.get_physical_address(address)
		.unwrap()
		.0;
	Return { status: 0, value }
}

extern "C" fn map_object(
	handle: usize,
	base: usize,
	offset_l: usize,
	offset_h_or_length: usize,
	length_or_rwx: usize,
	rwx: usize,
) -> Return {
	debug!("map_object");
	let (offset, _length, _rwx) = match mem::size_of_val(&offset_l) {
		4 => (
			(offset_h_or_length as u64) << 32 | offset_l as u64,
			length_or_rwx,
			rwx,
		),
		8 | 16 => (offset_l as u64, offset_h_or_length, length_or_rwx),
		s => unreachable!("unsupported usize size of {}", s),
	};
	let handle = handle as u32;
	let base = NonNull::new(base as *mut _);
	Process::current()
		.unwrap()
		.map_memory_object_2(handle, base, offset, RWX::RW)
		.map_or(
			Return {
				status: 1,
				value: 0,
			},
			|base| Return {
				status: 0,
				value: base.as_ptr() as usize,
			},
		)
}

extern "C" fn duplicate_handle(
	handle: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("duplicate_handle");
	let handle = handle as u32;

	Process::current()
		.unwrap()
		.duplicate_object_handle(handle)
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
	let time = Duration::from_micros(time.into());

	Thread::current()
		.unwrap()
		.set_sleep_until(Monotonic::now().saturating_add(time));
	Thread::yield_current();

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

extern "C" fn create_io_rings(
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

extern "C" fn submit_io(base: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	debug!("submit_io");
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

extern "C" fn wait_io(base: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	debug!("wait_io");
	let Some(base) = NonNull::new(base as *mut _) else {
		return Return { status: Error::InvalidData as usize, value: 0 }
	};
	Process::current().unwrap().wait_io_queue(base).map_or(
		Return {
			status: Error::Unknown as usize,
			value: 0,
		},
		|_| get_mono_time(),
	)
}

extern "C" fn kill_thread(
	handle: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
	_: usize,
) -> Return {
	debug!("kill_thread");
	// To keep things simple & safe, always switch to the CPU local stack & start running
	// the next thread, even if it isn't the most efficient way to do things.
	let Some(thread) = Process::current().unwrap().remove_thread(handle as u32) else {
		return Return {
			status: usize::MAX,
			value: 0,
		};
	};
	let thread = Arc::into_raw(thread);
	crate::arch::run_on_local_cpu_stack_noreturn!(destroy_thread, thread.cast());

	extern "C" fn destroy_thread(data: *const ()) -> ! {
		let thread = unsafe { Arc::from_raw(data.cast::<Thread>()) };

		crate::arch::amd64::clear_current_thread();

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
	struct D(*const Process, i32);
	let d = D(Arc::into_raw(Process::current().unwrap()), code as i32);
	crate::arch::run_on_local_cpu_stack_noreturn!(destroy_process, &d as *const _ as _);

	extern "C" fn destroy_process(data: *const ()) -> ! {
		let D(process, _code) = unsafe { data.cast::<D>().read() };
		let process = unsafe { Arc::from_raw(process) };

		crate::arch::amd64::clear_current_thread();

		unsafe {
			AddressSpace::activate_default();
		}

		// SAFETY: we switched to the CPU local stack and won't return to a stack of a thread
		// owned by this process. We also switched to the default address space.
		unsafe {
			process.destroy();
		}

		// SAFETY: there is no thread state to save.
		unsafe { scheduler::next_thread() }
	}
}

extern "C" fn create_root(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	Process::current()
		.unwrap()
		.add_object(Arc::new(crate::object_table::Root::new()))
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

extern "C" fn undefined(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	debug!("undefined");
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
