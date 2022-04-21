pub const ID_ALLOC: usize = 0;
pub const ID_DEALLOC: usize = 1;

pub const ID_ALLOC_DMA: usize = 3;
pub const ID_PHYSICAL_ADDRESS: usize = 4;
pub const ID_NEXT_TABLE: usize = 5;
pub const ID_MAP_OBJECT: usize = 9;
pub const ID_SLEEP: usize = 10;
pub const ID_READ: usize = 11;
pub const ID_CREATE_TABLE: usize = 13;
pub const ID_KILL_THREAD: usize = 14;
pub const ID_WAIT_THREAD: usize = 15;
pub const ID_EXIT: usize = 16;

pub const ID_DUPLICATE_HANDLE: usize = 18;
pub const ID_SPAWN_THREAD: usize = 19;
pub const ID_CREATE_IO_QUEUE: usize = 20;
pub const ID_PROCESS_IO_QUEUE: usize = 21;
pub const ID_WAIT_IO_QUEUE: usize = 22;

use crate::Page;
use core::arch::asm;
use core::fmt;
use core::mem::{self, MaybeUninit};
use core::num::NonZeroUsize;
use core::ptr::{self, NonNull};
use core::str;
use core::time::Duration;

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

pub type TableId = u32;
pub type Handle = u32;

#[derive(Clone)]
#[repr(C)]
pub struct TableInfo {
	name_len: u8,
	name: [u8; 255],
}

impl TableInfo {
	pub fn name(&self) -> &[u8] {
		&self.name[..usize::from(self.name_len)]
	}
}

impl Default for TableInfo {
	fn default() -> Self {
		Self {
			name_len: 0,
			name: [0; 255],
		}
	}
}

impl fmt::Debug for TableInfo {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct(stringify!(TableInfo))
			.field("name", &DebugLossy(self.name()))
			.finish()
	}
}

pub enum TableType {
	Streaming,
}

pub enum RWX {
	R = 0b100,
	W = 0b010,
	X = 0b001,
	RW = 0b110,
	RX = 0b101,
	RWX = 0b111,
}

macro_rules! syscall {
	(@INTERNAL $id:ident [$(in($reg:tt) $val:expr),*]) => {
		unsafe {
			let status @ value: usize;
			asm!(
				"syscall",
				in("eax") $id,
				$(in($reg) $val),*,
				lateout("rax") status,
				lateout("rdx") value,
				lateout("rcx") _,
				lateout("r11") _,
			);
			(status, value)
		}
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
}

#[inline]
pub fn alloc(
	base: Option<NonNull<Page>>,
	size: usize,
	rwx: RWX,
) -> Result<(NonNull<Page>, NonZeroUsize), (NonZeroUsize, usize)> {
	let base = base.map_or_else(core::ptr::null_mut, NonNull::as_ptr);
	ret2(syscall!(ID_ALLOC(base, size, rwx as usize))).map(|(status, value)| {
		(
			NonNull::new(value as *mut _).unwrap(),
			NonZeroUsize::new(status).unwrap(),
		)
	})
}

#[inline]
pub unsafe fn dealloc(
	base: NonNull<Page>,
	size: usize,
	dealloc_partial_start: bool,
	dealloc_partial_end: bool,
) -> Result<(), (NonZeroUsize, usize)> {
	let flags = (dealloc_partial_end as usize) << 1 | (dealloc_partial_start as usize);
	ret(syscall!(ID_DEALLOC(base.as_ptr(), size, flags))).map(|_| ())
}

#[inline]
pub fn alloc_dma(
	base: Option<NonNull<Page>>,
	size: usize,
) -> Result<(NonNull<Page>, NonZeroUsize), (NonZeroUsize, usize)> {
	ret2(syscall!(ID_ALLOC_DMA(
		base.map_or_else(ptr::null_mut, NonNull::as_ptr),
		size
	)))
	.map(|(status, value)| {
		(
			NonNull::new(value as *mut _).unwrap(),
			NonZeroUsize::new(status).unwrap(),
		)
	})
}

#[inline]
pub fn physical_address(base: NonNull<Page>) -> Result<usize, (NonZeroUsize, usize)> {
	ret(syscall!(ID_PHYSICAL_ADDRESS(base.as_ptr())))
}

#[inline]
pub fn next_table(id: Option<TableId>) -> Option<(TableId, TableInfo)> {
	let id = id.map_or(usize::MAX, |id| id.try_into().unwrap());
	let mut info = TableInfo::default();
	ret(syscall!(ID_NEXT_TABLE(id, &mut info)))
		.ok()
		.map(|value| (value as u32, info))
}

#[inline]
pub fn map_object(
	handle: Handle,
	base: Option<NonNull<Page>>,
	offset: u64,
	length: usize,
) -> Result<NonNull<Page>, (NonZeroUsize, usize)> {
	let base = base.map_or_else(core::ptr::null_mut, NonNull::as_ptr);
	ret(syscall!(ID_MAP_OBJECT(handle, base, offset, length)))
		.map(|v| NonNull::new(v as *mut _).unwrap())
}

#[inline]
pub fn sleep(duration: Duration) {
	match duration_to_micros(duration) {
		(l, None) => syscall!(ID_SLEEP(l)),
		(l, Some(h)) => syscall!(ID_SLEEP(l, h)),
	};
}

#[inline]
pub unsafe fn spawn_thread(
	start: unsafe extern "C" fn() -> !,
	stack: *const (),
) -> Result<Handle, (NonZeroUsize, usize)> {
	ret(syscall!(ID_SPAWN_THREAD(start, stack))).map(|h| h as Handle)
}

#[inline]
pub fn read(object: Handle, data: &mut [u8]) -> Result<usize, (NonZeroUsize, usize)> {
	// SAFETY: MaybeUninit has the same layout as data.
	let data = unsafe { mem::transmute(data) };
	read_uninit(object, data)
}

#[inline]
pub fn read_uninit(
	object: Handle,
	data: &mut [MaybeUninit<u8>],
) -> Result<usize, (NonZeroUsize, usize)> {
	ret(syscall!(ID_READ(object, data.as_mut_ptr(), data.len())))
}

#[inline]
pub fn duplicate_handle(handle: Handle) -> Result<Handle, (NonZeroUsize, usize)> {
	ret(syscall!(ID_DUPLICATE_HANDLE(handle))).map(|v| v as u32)
}

#[inline]
pub fn create_table(name: &[u8], ty: TableType) -> Result<Handle, (NonZeroUsize, usize)> {
	let ty = match ty {
		TableType::Streaming => 0,
	};
	ret(syscall!(ID_CREATE_TABLE(
		name.as_ptr(),
		name.len(),
		ty,
		ptr::null::<()>()
	)))
	.map(|v| v as u32)
}

#[inline]
pub fn create_io_queue(
	base: Option<NonNull<Page>>,
	request_p2size: u8,
	response_p2size: u8,
) -> Result<NonNull<Page>, (NonZeroUsize, usize)> {
	let base = base.map_or_else(ptr::null_mut, NonNull::as_ptr);
	let request_p2size = u32::from(request_p2size);
	let response_p2size = u32::from(response_p2size);
	ret(syscall!(ID_CREATE_IO_QUEUE(
		base,
		request_p2size,
		response_p2size
	)))
	.map(|v| NonNull::new(v as *mut _).unwrap())
}

#[inline]
pub fn process_io_queue(base: Option<NonNull<Page>>) -> Result<usize, (NonZeroUsize, usize)> {
	ret(syscall!(ID_PROCESS_IO_QUEUE(
		base.map_or(ptr::null_mut(), NonNull::as_ptr)
	)))
}

#[inline]
pub fn wait_io_queue(base: Option<NonNull<Page>>) -> Result<usize, (NonZeroUsize, usize)> {
	ret(syscall!(ID_WAIT_IO_QUEUE(
		base.map_or(ptr::null_mut(), NonNull::as_ptr)
	)))
}

#[inline]
pub fn kill_thread(handle: Handle) -> Result<(), (NonZeroUsize, usize)> {
	ret(syscall!(ID_KILL_THREAD(handle))).map(|_| ())
}

#[inline]
pub fn wait_thread(handle: Handle) -> Result<(), (NonZeroUsize, usize)> {
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

fn ret((status, value): (usize, usize)) -> Result<usize, (NonZeroUsize, usize)> {
	match NonZeroUsize::new(status) {
		None => Ok(value),
		Some(status) => Err((status, value)),
	}
}

fn ret2((status, value): (usize, usize)) -> Result<(usize, usize), (NonZeroUsize, usize)> {
	if (status as isize) < 0 {
		Err((NonZeroUsize::new(status).unwrap(), value))
	} else {
		Ok((status, value))
	}
}

fn duration_to_micros(t: Duration) -> (usize, Option<usize>) {
	let micros = u64::try_from(t.as_micros()).unwrap_or(u64::MAX);
	match mem::size_of::<usize>() {
		4 => (micros as usize, Some((micros >> 32) as usize)),
		8 => (micros as usize, None),
		_ => todo!(),
	}
}
