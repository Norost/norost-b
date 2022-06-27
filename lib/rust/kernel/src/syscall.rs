pub const ID_ALLOC: usize = 0;
pub const ID_DEALLOC: usize = 1;
pub const ID_MONOTONIC_TIME: usize = 2;

pub const ID_DO_IO: usize = 6;
pub const ID_HAS_SINGLE_OWNER: usize = 7;
pub const ID_NEW_OBJECT: usize = 8;
pub const ID_MAP_OBJECT: usize = 9;
pub const ID_SLEEP: usize = 10;

pub const ID_DESTROY_IO_QUEUE: usize = 13;
pub const ID_KILL_THREAD: usize = 14;
pub const ID_WAIT_THREAD: usize = 15;
pub const ID_EXIT: usize = 16;

pub const ID_SPAWN_THREAD: usize = 19;
pub const ID_CREATE_IO_QUEUE: usize = 20;
pub const ID_PROCESS_IO_QUEUE: usize = 21;
pub const ID_WAIT_IO_QUEUE: usize = 22;

use crate::{
	error, io,
	object::{NewObject, NewObjectArgs},
	time::Monotonic,
	Page,
};
use core::{
	arch::asm,
	fmt,
	num::NonZeroUsize,
	ptr::{self, NonNull},
	str,
	time::Duration,
};

pub struct ExitStatus(pub u32);

struct DebugLossy<'a>(&'a [u8]);

impl fmt::Debug for DebugLossy<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		use core::fmt::Write;
		let mut s = self.0;
		f.write_char('"')?;
		loop {
			match str::from_utf8(s) {
				Ok(s) => break s.escape_debug().try_for_each(|c| f.write_char(c))?,
				Err(e) => {
					str::from_utf8(&s[..e.valid_up_to()])
						.unwrap()
						.escape_debug()
						.try_for_each(|c| f.write_char(c))?;
					s = &s[e.valid_up_to()..];
				}
			}
		}
		f.write_char('"')
	}
}

pub type Handle = u32;

pub enum RWX {
	R = 0b100,
	W = 0b010,
	X = 0b001,
	RW = 0b110,
	RX = 0b101,
	RWX = 0b111,
}

#[allow(unused_macro_rules)]
macro_rules! syscall {
	(@INTERNAL $id:ident [$(in($reg:tt) $val:expr),*]) => {
		unsafe {
			let status @ value: usize;
			asm!(
				"syscall",
				in("eax") $id,
				$(in($reg) $val,)*
				lateout("rax") status,
				lateout("rdx") value,
				lateout("rcx") _,
				lateout("r11") _,
			);
			(status, value)
		}
	};
	($id:ident()) => {
		syscall!(@INTERNAL $id [])
	};
	($id:ident($a1:expr)) => {
		syscall!(@INTERNAL $id [in("rdi") $a1])
	};
	($id:ident($a1:expr, $a2:expr)) => {
		syscall!(@INTERNAL $id [in("rdi") $a1, in("rsi") $a2])
	};
	($id:ident($a1:expr, $a2:expr, $a3:expr)) => {
		syscall!(@INTERNAL $id [in("rdi") $a1, in("rsi") $a2, in("rdx") $a3])
	};
	($id:ident($a1:expr, $a2:expr, $a3:expr, $a4:expr)) => {
		// Use r10 instead of rcx as the latter gets overwritten by the syscall instruction
		syscall!(@INTERNAL $id [in("rdi") $a1, in("rsi") $a2, in("rdx") $a3, in("r10") $a4])
	};
	($id:ident($a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr)) => {
		// Ditto
		syscall!(@INTERNAL $id [in("rdi") $a1, in("rsi") $a2, in("rdx") $a3, in("r10") $a4, in("r8") $a5])
	};
	($id:ident($a1:expr, $a2:expr, $a3:expr, $a4:expr, $a5:expr, $a6:expr)) => {
		// Ditto
		syscall!(@INTERNAL $id [in("rdi") $a1, in("rsi") $a2, in("rdx") $a3, in("r10") $a4, in("r8") $a5, in("r9") $a6])
	};
}

#[inline]
pub fn alloc(
	base: Option<NonNull<Page>>,
	size: usize,
	rwx: RWX,
) -> error::Result<(NonNull<Page>, NonZeroUsize)> {
	let base = base.map_or_else(ptr::null_mut, NonNull::as_ptr);
	ret(syscall!(ID_ALLOC(base, size, rwx as usize))).map(|(status, value)| {
		// SAFETY: the kernel always returns a non-zero status (size) and value (base ptr).
		// If the kernel is buggy we're screwed anyways.
		unsafe {
			(
				NonNull::new_unchecked(value as *mut _),
				NonZeroUsize::new_unchecked(status),
			)
		}
	})
}

#[inline]
pub unsafe fn dealloc(base: NonNull<Page>, size: usize) -> error::Result<()> {
	ret(syscall!(ID_DEALLOC(base.as_ptr(), size))).map(|_| ())
}

#[inline]
pub fn monotonic_time() -> Monotonic {
	sys_to_mono(syscall!(ID_MONOTONIC_TIME()))
}

#[inline]
pub fn do_io(request: io::DoIo<'_>) -> error::Result<u64> {
	let (ty, h, a) = request.into_args();
	let h = usize::try_from(h).unwrap();
	let ty = usize::from(ty);
	#[cfg(target_pointer_width = "64")]
	ret(match a {
		io::RawDoIo::N0 => syscall!(ID_DO_IO(ty, h)),
		io::RawDoIo::N1(a) => syscall!(ID_DO_IO(ty, h, a)),
		io::RawDoIo::N2(a, b) => syscall!(ID_DO_IO(ty, h, a, b)),
		io::RawDoIo::N3(a, b, c) => syscall!(ID_DO_IO(ty, h, a, b, c)),
	})
	.map(|(_, v)| v as u64)
}

#[inline]
pub fn new_object(args: NewObject) -> error::Result<Handle> {
	use NewObjectArgs::*;
	let (ty, args) = args.into_args();
	ret(match args {
		N0 => syscall!(ID_NEW_OBJECT(ty)),
		N1(a) => syscall!(ID_NEW_OBJECT(ty, a)),
		N2(a, b) => syscall!(ID_NEW_OBJECT(ty, a, b)),
		N3(a, b, c) => syscall!(ID_NEW_OBJECT(ty, a, b, c)),
	})
	.map(|(_, h)| h as u32)
}

#[inline]
pub fn map_object(
	handle: Handle,
	base: Option<NonNull<Page>>,
	rwx: RWX,
	offset: usize,
	max_length: usize,
) -> error::Result<(NonNull<Page>, usize)> {
	let base = base.map_or_else(core::ptr::null_mut, NonNull::as_ptr);
	ret(syscall!(ID_MAP_OBJECT(
		handle,
		base,
		rwx as usize,
		offset,
		max_length
	)))
	.map(|(s, v)| (NonNull::new(v as *mut _).unwrap(), s))
}

#[inline]
pub fn sleep(duration: Duration) -> Monotonic {
	sys_to_mono(match duration_to_sys(duration) {
		(l, None) => syscall!(ID_SLEEP(l)),
		(l, Some(h)) => syscall!(ID_SLEEP(l, h)),
	})
}

#[inline]
pub unsafe fn spawn_thread(
	start: unsafe extern "C" fn() -> !,
	stack: *const (),
) -> error::Result<Handle> {
	ret(syscall!(ID_SPAWN_THREAD(start, stack))).map(|(_, h)| h as Handle)
}

#[inline]
pub fn create_io_queue(
	base: Option<NonNull<Page>>,
	request_p2size: u8,
	response_p2size: u8,
) -> error::Result<NonNull<Page>> {
	let base = base.map_or_else(ptr::null_mut, NonNull::as_ptr);
	let request_p2size = u32::from(request_p2size);
	let response_p2size = u32::from(response_p2size);
	ret(syscall!(ID_CREATE_IO_QUEUE(
		base,
		request_p2size,
		response_p2size
	)))
	.map(|(_, v)| NonNull::new(v as *mut _).unwrap())
}

#[inline]
pub unsafe fn destroy_io_queue(base: NonNull<Page>) -> error::Result<()> {
	ret(syscall!(ID_DESTROY_IO_QUEUE(base.as_ptr()))).map(|_| ())
}

#[inline]
pub fn process_io_queue(base: Option<NonNull<Page>>) -> error::Result<Monotonic> {
	ret(syscall!(ID_PROCESS_IO_QUEUE(
		base.map_or(ptr::null_mut(), NonNull::as_ptr)
	)))
	.map(sys_to_mono)
}

#[inline]
pub fn wait_io_queue(base: Option<NonNull<Page>>, timeout: Duration) -> error::Result<Monotonic> {
	let base = base.map_or(ptr::null_mut(), NonNull::as_ptr);
	ret(match duration_to_sys(timeout) {
		(l, None) => syscall!(ID_WAIT_IO_QUEUE(base, l)),
		(l, Some(h)) => syscall!(ID_WAIT_IO_QUEUE(base, l, h)),
	})
	.map(sys_to_mono)
}

#[inline]
pub fn kill_thread(handle: Handle) -> error::Result<()> {
	ret(syscall!(ID_KILL_THREAD(handle))).map(|_| ())
}

#[inline]
pub fn wait_thread(handle: Handle) -> error::Result<()> {
	ret(syscall!(ID_WAIT_THREAD(handle))).map(|_| ())
}

#[inline]
pub fn exit(code: i32) -> ! {
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_EXIT,
			in("edi") code,
			options(noreturn, nomem),
		);
	}
}

fn ret((status, value): (usize, usize)) -> error::Result<(usize, usize)> {
	error::result(status as isize).map(|status| (status as usize, value))
}

fn duration_to_sys(t: Duration) -> (usize, Option<usize>) {
	let ns = u64::try_from(t.as_nanos()).unwrap_or(u64::MAX);
	#[cfg(target_pointer_width = "32")]
	return (ns as usize, Some((ns >> 32) as usize));
	#[cfg(target_pointer_width = "64")]
	return (ns as usize, None);
}

fn sys_to_mono((_hi, lo): (usize, usize)) -> Monotonic {
	#[cfg(target_pointer_width = "32")]
	return Monotonic::from_nanos(((_hi as u64) << 32) | (lo as u64));
	#[cfg(target_pointer_width = "64")]
	return Monotonic::from_nanos(lo as u64);
}
