use crate::scheduler::process::Process;

#[repr(C)]
struct Return {
	status: usize,
	value: usize,
}

type Syscall = extern "C" fn(usize, usize, usize, usize, usize, usize) -> Return;

pub const SYSCALLS_LEN: usize = 3;
#[export_name = "syscall_table"]
static SYSCALLS: [Syscall; SYSCALLS_LEN] = [syslog, init_client_queue, push_client_queue];

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
