const ID_SYSLOG: usize = 0;

const ID_ALLOC_DMA: usize = 3;
const ID_PHYSICAL_ADDRESS: usize = 4;
const ID_NEXT_TABLE: usize = 5;
const ID_MAP_OBJECT: usize = 9;
const ID_SLEEP: usize = 10;
const ID_READ: usize = 11;
const ID_CREATE_TABLE: usize = 13;

const ID_DUPLICATE_HANDLE: usize = 18;
const ID_SPAWN_THREAD: usize = 19;
const ID_CREATE_IO_QUEUE: usize = 20;
const ID_PROCESS_IO_QUEUE: usize = 21;
const ID_WAIT_IO_QUEUE: usize = 22;

use crate::Page;
use core::alloc::Layout;
use core::arch::asm;
use core::fmt;
use core::intrinsics;
use core::marker::PhantomData;
use core::mem::{self, MaybeUninit};
use core::num::NonZeroUsize;
use core::ptr::{self, NonNull};
use core::str;
use core::time::Duration;

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

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Slice<'a, T> {
	ptr: NonNull<T>,
	len: usize,
	_marker: PhantomData<&'a T>,
}

impl<'a, T> Slice<'a, T> {
	/// # Safety
	///
	/// `ptr` and `len` must be valid.
	pub unsafe fn unchecked_as_slice(&self) -> &'a [T] {
		unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
	}

	pub fn len(&self) -> usize {
		self.len
	}
}

impl<'a, T> From<&[T]> for Slice<'a, T> {
	fn from(s: &[T]) -> Self {
		Self {
			ptr: NonNull::from(s).as_non_null_ptr(),
			len: s.len(),
			_marker: PhantomData,
		}
	}
}

impl<'a, T, const N: usize> From<&'a [T; N]> for Slice<'a, T> {
	fn from(s: &[T; N]) -> Self {
		Self {
			ptr: NonNull::new(s.as_ptr() as *mut _).unwrap(),
			len: s.len(),
			_marker: PhantomData,
		}
	}
}

impl<'a, T> Default for Slice<'a, T> {
	fn default() -> Self {
		Self {
			ptr: NonNull::new(Layout::new::<T>().align() as *mut _)
				.unwrap_or(NonNull::new(1 as *mut _).unwrap()),
			len: 0,
			_marker: PhantomData,
		}
	}
}

struct ByteStr<'a>(&'a [u8]);

impl fmt::Debug for ByteStr<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match core::str::from_utf8(self.0) {
			Ok(s) => s.fmt(f),
			Err(_) => format_args!("{:?}", self).fmt(f),
		}
	}
}

pub enum TableType {
	Streaming,
}

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct TableHandle(usize);

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct Events(u32);

#[optimize(size)]
#[inline]
pub fn syslog(s: &[u8]) -> Result<usize, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_SYSLOG,
			in("rdi") s.as_ptr(),
			in("rsi") s.len(),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	ret(status, value)
}

#[inline]
pub fn alloc_dma(
	base: Option<NonNull<Page>>,
	size: usize,
) -> Result<NonNull<Page>, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_ALLOC_DMA,
			in("rdi") base.map_or_else(core::ptr::null_mut, NonNull::as_ptr),
			in("rsi") size,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	ret(status, value).map(|p| NonNull::new(p as *mut _).unwrap_or_else(|| intrinsics::abort()))
}

#[inline]
pub fn physical_address(base: NonNull<Page>) -> Result<usize, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_PHYSICAL_ADDRESS,
			in("rdi") base.as_ptr(),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	ret(status, value)
}

#[inline]
pub fn next_table(id: Option<TableId>) -> Option<(TableId, TableInfo)> {
	let (status, value): (usize, usize);
	let mut info = TableInfo::default();
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_NEXT_TABLE,
			in("rdi") id.map_or(usize::MAX, |id| id.try_into().unwrap()),
			in("rsi") &mut info,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	(status == 0).then(|| (value as u32, info))
}

#[inline]
pub fn map_object(
	handle: Handle,
	base: Option<NonNull<Page>>,
	offset: u64,
	length: usize,
) -> Result<NonNull<Page>, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_MAP_OBJECT,
			in("rdi") handle,
			in("rsi") base.map_or_else(core::ptr::null_mut, NonNull::as_ptr),
			in("rdx") offset,
			in("r10") length,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value).map(|v| NonNull::new(v as *mut _).unwrap())
}

#[inline]
pub fn sleep(duration: Duration) {
	let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_SLEEP,
			in("rdi") micros,
			// Ignore failures and pretend the sleep terminated early
			lateout("rax") _,
			lateout("rdx") _,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
}

#[inline]
pub unsafe fn spawn_thread(
	start: unsafe extern "C" fn() -> !,
	stack: *const (),
) -> Result<usize, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_SPAWN_THREAD,
			in("rdi") start,
			in("rsi") stack,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	ret(status, value)
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
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_READ,
			in("rdi") object,
			in("rsi") data.as_mut_ptr(),
			in("rdx") data.len(),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	ret(status, value)
}

#[inline]
pub fn duplicate_handle(handle: Handle) -> Result<Handle, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_DUPLICATE_HANDLE,
			in("rdi") handle,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	ret(status, value).map(|v| v as u32)
}

#[inline]
pub fn create_table(name: &[u8], ty: TableType) -> Result<Handle, (NonZeroUsize, usize)> {
	let ty = match ty {
		TableType::Streaming => 0,
	};
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_CREATE_TABLE,
			in("rdi") name.as_ptr(),
			in("rsi") name.len(),
			in("rdx") ty,
			in("rcx") ptr::null::<()>(),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value).map(|v| v as u32)
}

#[inline]
pub fn create_io_queue(
	base: Option<NonNull<Page>>,
	request_p2size: u8,
	response_p2size: u8,
) -> Result<NonNull<Page>, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_CREATE_IO_QUEUE,
			in("rdi") base.map_or(ptr::null_mut(), NonNull::as_ptr),
			in("esi") u32::from(request_p2size),
			in("edx") u32::from(response_p2size),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value).map(|v| NonNull::new(v as *mut _).unwrap())
}

#[inline]
pub fn process_io_queue(base: Option<NonNull<Page>>) -> Result<usize, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_PROCESS_IO_QUEUE,
			in("rdi") base.map_or(ptr::null_mut(), NonNull::as_ptr),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value)
}

#[inline]
pub fn wait_io_queue(base: Option<NonNull<Page>>) -> Result<usize, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_WAIT_IO_QUEUE,
			in("rdi") base.map_or(ptr::null_mut(), NonNull::as_ptr),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value)
}

fn ret(status: usize, value: usize) -> Result<usize, (NonZeroUsize, usize)> {
	match NonZeroUsize::new(status) {
		None => Ok(value),
		Some(status) => Err((status, value)),
	}
}
