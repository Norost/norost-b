use crate::driver;
use crate::memory::{frame, Page, r#virtual::RWX};
use crate::scheduler::{process::Process, syscall::frame::DMAFrame};
use core::ptr::NonNull;
use alloc::boxed::Box;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Return {
	pub status: usize,
	pub value: usize,
}

type Syscall = extern "C" fn(usize, usize, usize, usize, usize, usize) -> Return;

pub const SYSCALLS_LEN: usize = 7;
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

#[allow(dead_code)]
extern "C" fn undefined(_: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Return {
	Return {
		status: usize::MAX,
		value: 0,
	}
}
