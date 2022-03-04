const ID_SYSLOG: usize = 0;

const ID_ALLOC_DMA: usize = 3;
const ID_PHYSICAL_ADDRESS: usize = 4;
const ID_NEXT_TABLE: usize = 5;
const ID_QUERY_TABLE: usize = 6;
const ID_QUERY_NEXT: usize = 7;
const ID_OPEN_OBJECT: usize = 8;
const ID_MAP_OBJECT: usize = 9;
const ID_SLEEP: usize = 10;
const ID_READ: usize = 11;
const ID_WRITE: usize = 12;
const ID_CREATE_TABLE: usize = 13;

const ID_TAKE_TABLE_JOB: usize = 15;
const ID_FINISH_TABLE_JOB: usize = 16;

use crate::Page;
use core::alloc::Layout;
use core::arch::asm;
use core::fmt;
use core::marker::PhantomData;
use core::mem::{self, MaybeUninit};
use core::num::NonZeroUsize;
use core::ptr::NonNull;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Id(pub u64);

impl Default for Id {
	fn default() -> Self {
		Self(u64::MAX)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct TableId(pub u32);

impl Default for TableId {
	fn default() -> Self {
		Self(u32::MAX)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Handle(pub usize);

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

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct QueryHandle(usize);

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
		core::slice::from_raw_parts(self.ptr.as_ptr(), self.len)
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

#[repr(C)]
pub struct ObjectInfo<'a> {
	pub id: Id,
	tags_len: u8,
	tags_offsets: [u32; 255],
	string_buffer: &'a mut [u8],
}

impl<'a> ObjectInfo<'a> {
	pub fn new(string_buffer: &'a mut [u8]) -> Self {
		Self {
			string_buffer,
			..Default::default()
		}
	}

	pub fn tag(&'a self, index: usize) -> &'a [u8] {
		let index = self.tags_offsets[index] as usize;
		let len = usize::from(self.string_buffer[index]);
		&self.string_buffer[index + 1..index + 1 + len]
	}

	pub fn tags_count(&self) -> usize {
		self.tags_len.into()
	}
}

impl Default for ObjectInfo<'_> {
	fn default() -> Self {
		Self {
			id: Default::default(),
			tags_len: 0,
			tags_offsets: [0; 255],
			string_buffer: &mut [],
		}
	}
}

impl fmt::Debug for ObjectInfo<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		use core::cell::Cell;

		struct S<'a, I: Iterator<Item = &'a [u8]>>(Cell<Option<I>>);

		impl<'a, I> fmt::Debug for S<'a, I>
		where
			I: Iterator<Item = &'a [u8]>,
		{
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				let s = self.0.take().unwrap();
				let mut f = f.debug_list();
				s.for_each(|e| {
					f.entry(&ByteStr(e));
				});
				f.finish()
			}
		}

		let mut f = f.debug_struct(stringify!(ObjectInfo));
		f.field("id", &self.id);
		f.field(
			"tags",
			&S(Cell::new(Some((0..self.tags_count()).map(|i| self.tag(i))))),
		);
		f.finish()
	}
}

struct ByteStr<'a>(&'a [u8]);

impl fmt::Debug for ByteStr<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match core::str::from_utf8(self.0) {
			Ok(s) => s.fmt(f),
			Err(_) => format_args!("{:?}", self.0).fmt(f),
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

#[derive(Debug, Default)]
#[repr(C)]
pub struct Job<'a> {
	pub ty: u8,
	pub flags: [u8; 3],
	pub job_id: JobId,
	pub buffer_size: u32,
	pub operation_size: u32,
	pub object_id: Id,
	pub buffer: Option<NonNull<u8>>,
	_marker: PhantomData<&'a ()>,
}

impl<'a> Job<'a> {
	pub const OPEN: u8 = 0;
	pub const READ: u8 = 1;
	pub const WRITE: u8 = 2;

	pub unsafe fn data(&self) -> &'a [u8] {
		core::slice::from_raw_parts(
			self.buffer.unwrap().as_ptr(),
			self.operation_size.try_into().unwrap(),
		)
	}
}

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct JobId(u32);

impl Default for JobId {
	fn default() -> Self {
		Self(u32::MAX)
	}
}

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
pub fn alloc_dma(base: Option<NonNull<Page>>, size: usize) -> Result<usize, (NonZeroUsize, usize)> {
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
	ret(status, value)
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
			in("rdi") id.map_or(usize::MAX, |id| id.0.try_into().unwrap()),
			in("rsi") &mut info,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	(status == 0).then(|| (TableId(value as u32), info))
}

#[inline]
pub fn query_table(
	id: TableId,
	name: Option<&[u8]>,
	tags: &[Slice<'_, u8>],
) -> Result<QueryHandle, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_QUERY_TABLE,
			in("rdi") usize::try_from(id.0).unwrap(),
			in("rsi") name.map_or_else(core::ptr::null, |n| n.as_ptr()),
			in("rdx") name.map_or(0, |n| n.len()),
			in("r10") tags.as_ptr(),
			in("r8") tags.len(),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value).map(|v| QueryHandle(v))
}

#[inline]
pub fn query_next(
	query: QueryHandle,
	info: &mut ObjectInfo<'_>,
) -> Result<(), (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_QUERY_NEXT,
			in("rdi") query.0,
			in("rsi") info,
			in("rdx") info.string_buffer.as_ptr(),
			in("r10") info.string_buffer.len(),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value).map(|_| ())
}

#[inline]
pub fn open(table_id: TableId, id: Id) -> Result<Handle, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_OPEN_OBJECT,
			in("rdi") table_id.0,
			in("rsi") id.0,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value).map(|v| Handle(v))
}

#[deprecated(note = "use open()")]
#[inline]
pub fn open_object(table_id: TableId, id: Id) -> Result<Handle, (NonZeroUsize, usize)> {
	open(table_id, id)
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
			in("rdi") handle.0,
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
			in("rdi") object.0,
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
pub fn write(object: Handle, data: &[u8]) -> Result<usize, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_WRITE,
			in("rdi") object.0,
			in("rsi") data.as_ptr(),
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
pub fn create_table(name: &str, ty: TableType) -> Result<Handle, (NonZeroUsize, usize)> {
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
			in("rcx") core::ptr::null::<()>(),
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value).map(Handle)
}

#[inline]
pub fn take_table_job<'a>(
	handle: Handle,
	buffer: &'a mut [u8],
) -> Result<Job<'a>, (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	let mut job = Job::default();
	job.buffer_size = buffer.len().try_into().unwrap();
	job.buffer = NonNull::new(buffer.as_mut_ptr());
	//syslog!("{:#?}", &job);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_TAKE_TABLE_JOB,
			in("rdi") handle.0,
			in("rsi") &mut job,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value).map(|_| job)
}

#[inline]
pub fn finish_table_job(handle: Handle, mut job: Job<'_>) -> Result<(), (NonZeroUsize, usize)> {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_FINISH_TABLE_JOB,
			in("rdi") handle.0,
			in("rsi") &mut job,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		)
	}
	ret(status, value).map(|_| ())
}

pub struct SysLog {
	buffer: [u8; 127],
	pub index: u8,
}

impl SysLog {
	#[doc(hidden)]
	#[optimize(size)]
	pub fn flush(&mut self) {
		// Ignore errors because what can we do? Panic won't do us any
		// good either.
		let _ = syslog(&self.buffer[..usize::from(self.index)]);
		self.index = 0;
	}

	#[doc(hidden)]
	#[optimize(size)]
	pub fn write_raw(&mut self, s: &[u8]) {
		for &c in s {
			if c == b'\n' || usize::from(self.index) >= self.buffer.len() {
				self.flush();
			}
			if c != b'\n' {
				self.buffer[usize::from(self.index)] = c;
				self.index += 1;
			}
		}
	}
}

impl fmt::Write for SysLog {
	#[optimize(size)]
	fn write_str(&mut self, s: &str) -> fmt::Result {
		self.write_raw(s.as_bytes());
		Ok(())
	}
}

// No Default impl for [u8; 127] :(
impl Default for SysLog {
	#[optimize(size)]
	fn default() -> Self {
		Self {
			buffer: [0; 127],
			index: 0,
		}
	}
}

impl Drop for SysLog {
	#[optimize(size)]
	fn drop(&mut self) {
		if self.index > 0 {
			self.flush();
		}
	}
}

#[macro_export]
macro_rules! syslog {
	($($arg:tt)*) => {
		{
			use core::fmt::Write;
			use $crate::syscall::SysLog;
			let _ = write!(SysLog::default(), $($arg)*);
		}
	};
}

fn ret(status: usize, value: usize) -> Result<usize, (NonZeroUsize, usize)> {
	match NonZeroUsize::new(status) {
		None => Ok(value),
		Some(status) => Err((status, value)),
	}
}
