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

type Syscall = fn(usize, usize, usize, usize, usize, usize) -> Return;

const SYSCALLS_COUNT: usize = 0;
static SYSCALLS: [Syscall; SYSCALLS_COUNT] = [
	create_process,
	kill_process,
	spawn_thread,
	hop_thread,
	exit_thread,
];

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

}

/// Create a new thread in another process.
fn call_new(port: usize, port_len: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	if port_len == 0 {
		// Anonymous, numeric ID
	} else {
		// Public, string ID
	}
}

/// Move the current thread to another process.
fn call(port: usize, port_len: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	if port_len == 0 {
		// Anonymous, numeric ID
	} else {
		// Public, string ID
	}
}

/// Return to the calling process
fn r#return(port: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {

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
