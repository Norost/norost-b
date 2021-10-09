use crate::ipc;
use crate::scheduler::process::Process;

macro_rules! syscall {
	{
		$(#[$outer:meta])*
		[$task:pat]
		$fn:ident($a0:pat, $a1:pat, $a2:pat, $a3:pat, $a4:pat, $a5:pat)
		$block:block
	} => {
		$(#[$outer])*
		pub extern "C" fn $fn($a0: usize, $a1: usize, $a2: usize, $a3: usize, $a4: usize, $a5: usize) -> Return {
			$block
		}
	};
}

#[repr(C)]
struct Return {
	status: usize,
	value: usize,
}

type Syscall = extern "C" fn(usize, usize, usize, usize, usize, usize) -> Return;

pub const SYSCALLS_LEN: usize = 3;
#[export_name = "syscall_table"]
static SYSCALLS: [Syscall; SYSCALLS_LEN] = [
	syslog,
	init_client_queue,
	push_client_queue,
];

extern "C" fn syslog(ptr: usize, len: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	// SAFETY: FIXME
	let s = unsafe { core::slice::from_raw_parts(ptr as *const u8, len) };
	info!("[user log] {}", core::str::from_utf8(s).unwrap_or("<illegible>"));
	Return {
		status: 0,
		value: len,
	}
}

extern "C" fn init_client_queue(address: usize, submission_p2size: usize, completion_p2size: usize, _: usize, _: usize, _: usize) -> Return {
	Process::current()
		.init_client_queue(address as *mut _, submission_p2size as u8, completion_p2size as u8)
		.unwrap();
	Return {
		status: 0,
		value: 0,
	}
}

extern "C" fn push_client_queue(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	Process::current()
		.poll_client_queue()
		.unwrap();
	Return {
		status: 0,
		value: 0,
	}
}

/// Create a new process. This process will automatically have one running thread to initialize
/// the process.
fn create_process(memory_maps: usize, memory_maps_count: usize, _: usize, _: usize, _: usize, _: usize) {

}

/// Kill a process. `-1` will kill the current process.
///
/// All threads in the process will be killed. All other processes with pending ports will be
/// notified.
fn kill_process(pid: usize, _: usize, _: usize, _: usize, _: usize, _: usize) {

}

/// Create a new thread in the same process.
fn new(program_counter: usize, registers: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	todo!()
}

/// Destroy the current thread.
fn exit(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {

}

/// Allocate a range of private pages.
fn allocate_memory(address: usize, size: usize, _: usize, _: usize, _: usize, _: usize) -> Return {

}

/// Unmap a memory range, which may deallocate the associated pages.
fn unmap_memory(address: usize, size: usize, _: usize, _: usize, _: usize, _: usize) -> Return {

}

/// Make a region of memory shareable. The memory may not already be shareable, i.e. a page can
/// only belong to one shareable set.
fn make_shareable(address: usize, size: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	
}

/// Attempt to a region of memory private again. This is useful to determine whether it makes
/// sense to evict a set of cached data.
fn make_private(address: usize, size: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	
}
