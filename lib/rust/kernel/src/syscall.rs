// Syscall IDs are assigned such that the 8 most commonly used ones appear first
// (8 * 8 = 64 = 1 cache line)

pub const ID_ALLOC: usize = 0;
pub const ID_DEALLOC: usize = 1;
pub const ID_NEW_OBJECT: usize = 2;
pub const ID_MAP_OBJECT: usize = 3;
pub const ID_DO_IO: usize = 4;
pub const ID_POLL_IO_QUEUE: usize = 5;
pub const ID_WAIT_IO_QUEUE: usize = 6;
pub const ID_MONOTONIC_TIME: usize = 7;

pub const ID_SLEEP: usize = 8;
pub const ID_EXIT: usize = 9;
pub const ID_SPAWN_THREAD: usize = 10;
pub const ID_WAIT_THREAD: usize = 11;
pub const ID_EXIT_THREAD: usize = 12;
pub const ID_CREATE_IO_QUEUE: usize = 13;
pub const ID_DESTROY_IO_QUEUE: usize = 14;

use {
	crate::{
		error, io,
		object::{NewObject, NewObjectArgs},
		time::Monotonic,
		Page,
	},
	core::{
		arch::asm,
		fmt,
		num::NonZeroUsize,
		ptr::{self, NonNull},
		str,
		time::Duration,
	},
};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RWX {
	R = 0b100,
	W = 0b010,
	X = 0b001,
	RW = 0b110,
	RX = 0b101,
	RWX = 0b111,
}

impl RWX {
	#[inline]
	pub fn from_flags(r: bool, w: bool, x: bool) -> Result<RWX, IncompatibleRWXFlags> {
		match (r, w, x) {
			(true, false, false) => Ok(Self::R),
			(false, true, false) => Ok(Self::W),
			(false, false, true) => Ok(Self::X),
			(true, true, false) => Ok(Self::RW),
			(true, false, true) => Ok(Self::RX),
			(true, true, true) => Ok(Self::RWX),
			_ => Err(IncompatibleRWXFlags),
		}
	}

	#[inline]
	pub fn is_subset_of(&self, superset: Self) -> bool {
		self.intersection(superset) == Some(*self)
	}

	#[inline]
	pub fn intersection(&self, with: Self) -> Option<Self> {
		Self::from_flags(
			self.r() && with.r(),
			self.w() && with.w(),
			self.x() && with.x(),
		)
		.ok()
	}

	#[inline]
	pub fn into_raw(self) -> u8 {
		self as _
	}

	#[inline]
	pub fn try_from_raw(rwx: u8) -> Option<Self> {
		Some(match rwx {
			0b100 => Self::R,
			0b010 => Self::W,
			0b001 => Self::X,
			0b110 => Self::RW,
			0b101 => Self::RX,
			0b111 => Self::RWX,
			_ => return None,
		})
	}

	#[inline]
	pub fn r(&self) -> bool {
		match self {
			Self::R | Self::RW | Self::RX | Self::RWX => true,
			Self::W | Self::X => false,
		}
	}

	#[inline]
	pub fn w(&self) -> bool {
		match self {
			Self::W | Self::RW | Self::RWX => true,
			Self::R | Self::X | Self::RX => false,
		}
	}

	#[inline]
	pub fn x(&self) -> bool {
		match self {
			Self::X | Self::RX | Self::RWX => true,
			Self::R | Self::W | Self::RW => false,
		}
	}
}

#[derive(Debug)]
pub struct IncompatibleRWXFlags;

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
	Monotonic::from_nanos(crate::vsyscall::vsyscall_data().time_info.now_nanos())
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
pub fn new_object(args: NewObject) -> error::Result<(Handle, Handle)> {
	use NewObjectArgs::*;
	let (ty, args) = args.into_args();
	ret(match args {
		N0 => syscall!(ID_NEW_OBJECT(ty)),
		N1(a) => syscall!(ID_NEW_OBJECT(ty, a)),
		N2(a, b) => syscall!(ID_NEW_OBJECT(ty, a, b)),
		N3(a, b, c) => syscall!(ID_NEW_OBJECT(ty, a, b, c)),
	})
	.map(|(a, b)| (a as _, b as _))
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
pub fn sleep(duration: Duration) {
	// Assume sleep does not fail to reduce binary bloat a bit.
	// (It can't realistically fail without other stuff being broken too anyways)
	let _ = match duration_to_sys(duration) {
		(l, None) => syscall!(ID_SLEEP(l)),
		(l, Some(h)) => syscall!(ID_SLEEP(l, h)),
	};
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
pub fn process_io_queue(base: Option<NonNull<Page>>) -> error::Result<()> {
	ret(syscall!(ID_POLL_IO_QUEUE(
		base.map_or(ptr::null_mut(), NonNull::as_ptr)
	)))
	.map(|_| ())
}

#[inline]
pub fn wait_io_queue(base: Option<NonNull<Page>>, timeout: Duration) -> error::Result<()> {
	let base = base.map_or(ptr::null_mut(), NonNull::as_ptr);
	ret(match duration_to_sys(timeout) {
		(l, None) => syscall!(ID_WAIT_IO_QUEUE(base, l)),
		(l, Some(h)) => syscall!(ID_WAIT_IO_QUEUE(base, l, h)),
	})
	.map(|_| ())
}

#[inline]
pub fn exit_thread() -> error::Result<()> {
	ret(syscall!(ID_EXIT_THREAD())).map(|_| ())
}

#[inline]
pub fn wait_thread(handle: Handle) -> error::Result<()> {
	ret(syscall!(ID_WAIT_THREAD(handle))).map(|_| ())
}

#[inline]
pub fn exit(code: u8) -> ! {
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_EXIT,
			in("edi") u32::from(code),
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
