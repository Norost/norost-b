pub use norostb_kernel::{
	error::{Error, Result},
	io::SeekFrom,
	object::NewObject,
	syscall::RWX,
	Handle,
};

use crate::RefObject;
use core::{
	mem::{self, MaybeUninit},
	ptr::NonNull,
	sync::atomic::Ordering,
};
use norostb_kernel::{io::DoIo, syscall};

macro_rules! transmute_handle {
	($fn:ident, $set_fn:ident -> $handle:ident) => {
		#[inline(always)]
		pub fn $fn() -> Option<RefObject<'static>> {
			let h = crate::globals::GLOBALS
				.get_ref()
				.$handle
				.load(Ordering::Relaxed);
			(h != Handle::MAX).then(|| RefObject::from_raw(h))
		}

		#[inline(always)]
		pub fn $set_fn(h: Option<RefObject<'static>>) {
			let h = h.map_or(Handle::MAX, |h| h.into_raw());
			// SAFETY: $handle is only set once at the start of the program
			crate::globals::GLOBALS
				.get_ref()
				.$handle
				.store(h, Ordering::Relaxed);
		}
	};
}

transmute_handle!(stdin, set_stdin -> stdin_handle);
transmute_handle!(stdout, set_stdout -> stdout_handle);
transmute_handle!(stderr, set_stderr -> stderr_handle);
transmute_handle!(file_root, set_file_root -> file_root_handle);
transmute_handle!(net_root, set_net_root -> net_root_handle);
transmute_handle!(process_root, set_process_root -> process_root_handle);

#[derive(Copy, Clone)]
pub struct IoSlice<'a>(&'a [u8]);

impl<'a> IoSlice<'a> {
	#[inline]
	pub fn new(buf: &'a [u8]) -> IoSlice<'a> {
		IoSlice(buf)
	}

	#[inline]
	pub fn advance(&mut self, n: usize) {
		self.0 = &self.0[n..]
	}

	#[inline]
	pub fn as_slice(&self) -> &[u8] {
		self.0
	}
}

pub struct IoSliceMut<'a>(&'a mut [u8]);

impl<'a> IoSliceMut<'a> {
	#[inline]
	pub fn new(buf: &'a mut [u8]) -> IoSliceMut<'a> {
		IoSliceMut(buf)
	}

	#[inline]
	pub fn advance(&mut self, n: usize) {
		let slice = mem::replace(&mut self.0, &mut []);
		let (_, remaining) = slice.split_at_mut(n);
		self.0 = remaining;
	}

	#[inline]
	pub fn as_slice(&self) -> &[u8] {
		self.0
	}

	#[inline]
	pub fn as_mut_slice(&mut self) -> &mut [u8] {
		self.0
	}
}

#[inline(always)]
pub fn read(handle: Handle, buf: &mut [u8]) -> Result<usize> {
	// SAFETY: the kernel won't deinitialize unread bytes
	read_uninit(handle, unsafe { mem::transmute(buf) })
}

#[inline(always)]
pub fn read_uninit(handle: Handle, buf: &mut [MaybeUninit<u8>]) -> Result<usize> {
	syscall::do_io(DoIo::Read {
		handle,
		buf,
		peek: false,
	})
	.map(|v| v as _)
}

#[inline(always)]
pub fn peek(handle: Handle, buf: &mut [u8]) -> Result<usize> {
	// SAFETY: the kernel won't deinitialize unread bytes
	peek_uninit(handle, unsafe { mem::transmute(buf) })
}

#[inline(always)]
pub fn peek_uninit(handle: Handle, buf: &mut [MaybeUninit<u8>]) -> Result<usize> {
	syscall::do_io(DoIo::Read {
		handle,
		buf,
		peek: true,
	})
	.map(|v| v as _)
}

#[inline(always)]
pub fn write(handle: Handle, data: &[u8]) -> Result<usize> {
	syscall::do_io(DoIo::Write { handle, data }).map(|v| v as _)
}

#[inline(always)]
pub fn open(handle: Handle, path: &[u8]) -> Result<Handle> {
	syscall::do_io(DoIo::Open { handle, path }).map(|v| v as _)
}

#[inline(always)]
pub fn create(handle: Handle, path: &[u8]) -> Result<Handle> {
	syscall::do_io(DoIo::Create { handle, path }).map(|v| v as _)
}

#[inline(always)]
pub fn seek(handle: Handle, from: SeekFrom) -> Result<u64> {
	syscall::do_io(DoIo::Seek { handle, from })
}

#[inline(always)]
pub fn share(handle: Handle, share: Handle) -> Result<u64> {
	syscall::do_io(DoIo::Share { handle, share })
}

#[inline(always)]
pub fn new_object(args: NewObject) -> Result<Handle> {
	syscall::new_object(args)
}

#[inline]
pub fn map_object(
	handle: Handle,
	base: Option<NonNull<u8>>,
	rwx: RWX,
	offset: usize,
	max_length: usize,
) -> Result<(NonNull<u8>, usize)> {
	syscall::map_object(handle, base.map(NonNull::cast), rwx, offset, max_length)
		.map(|(b, l)| (b.cast(), l))
}

#[inline]
pub fn close(handle: Handle) {
	let _ = syscall::do_io(DoIo::Close { handle });
}
