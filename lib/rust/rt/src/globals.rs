use crate::Handle;
use core::{mem, sync::atomic::AtomicPtr};

#[linkage = "weak"]
#[export_name = "__rt_globals"]
static GLOBALS_PTR: AtomicPtr<Globals> = AtomicPtr::new(unsafe { &GLOBALS_VAL as *const _ as _ });

static mut GLOBALS_VAL: Globals = Globals {
	stdin_handle: Handle::MAX,
	stdout_handle: Handle::MAX,
	stderr_handle: Handle::MAX,
	file_root_handle: Handle::MAX,
	net_root_handle: Handle::MAX,
	process_root_handle: Handle::MAX,
};

pub(crate) static GLOBALS: GlobalsDeref = GlobalsDeref;

pub(crate) struct GlobalsDeref;

impl GlobalsDeref {
	pub(crate) unsafe fn get_mut(&self) -> &mut Globals {
		unsafe { mem::transmute_copy(&GLOBALS_PTR) }
	}

	pub(crate) unsafe fn get_ref(&self) -> &Globals {
		unsafe { mem::transmute_copy(&GLOBALS_PTR) }
	}
}

pub(crate) struct Globals {
	pub stdin_handle: Handle,
	pub stdout_handle: Handle,
	pub stderr_handle: Handle,
	pub file_root_handle: Handle,
	pub net_root_handle: Handle,
	pub process_root_handle: Handle,
}
