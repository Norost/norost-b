use crate::{AtomicHandle, Handle};
use core::{mem, sync::atomic::AtomicPtr};

#[linkage = "weak"]
#[export_name = "__rt_globals"]
static GLOBALS_PTR: AtomicPtr<Globals> = AtomicPtr::new(unsafe { &GLOBALS_VAL as *const _ as _ });

static mut GLOBALS_VAL: Globals = Globals {
	stdin_handle: AtomicHandle::new(Handle::MAX),
	stdout_handle: AtomicHandle::new(Handle::MAX),
	stderr_handle: AtomicHandle::new(Handle::MAX),
	file_root_handle: AtomicHandle::new(Handle::MAX),
	net_root_handle: AtomicHandle::new(Handle::MAX),
	process_root_handle: AtomicHandle::new(Handle::MAX),
};

pub(crate) static GLOBALS: GlobalsDeref = GlobalsDeref;

pub(crate) struct GlobalsDeref;

impl GlobalsDeref {
	pub(crate) fn get_ref(&self) -> &Globals {
		unsafe { mem::transmute_copy(&GLOBALS_PTR) }
	}
}

pub(crate) struct Globals {
	pub stdin_handle: AtomicHandle,
	pub stdout_handle: AtomicHandle,
	pub stderr_handle: AtomicHandle,
	pub file_root_handle: AtomicHandle,
	pub net_root_handle: AtomicHandle,
	pub process_root_handle: AtomicHandle,
}
