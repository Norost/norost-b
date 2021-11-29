const ID_SYSLOG: usize = 0;
const ID_PCI_MAP_ANY: usize = 3;
const ID_PCI_MAP_BAR: usize = 4;
const ID_ALLOC_DMA: usize = 5;
const ID_PHYSICAL_ADDRESS: usize = 6;

use crate::Page;
use core::num::NonZeroUsize;
use core::ptr::NonNull;

type Result = core::result::Result<usize, (NonZeroUsize, usize)>;

#[optimize(size)]
#[inline]
pub extern "C" fn syslog(s: &[u8]) -> Result {
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
pub extern "C" fn pci_map_any(id: u32, address: *const ()) -> Result {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_PCI_MAP_ANY,
			in("edi") id,
			in("rsi") address,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	ret(status, value)
}

#[inline]
pub extern "C" fn pci_map_bar(handle: u32, bar: u8, address: *const ()) -> Result {
	let (status, value): (usize, usize);
	unsafe {
		asm!(
			"syscall",
			in("eax") ID_PCI_MAP_BAR,
			in("edi") handle,
			in("sil") bar,
			in("rdx") address,
			lateout("rax") status,
			lateout("rdx") value,
			lateout("rcx") _,
			lateout("r11") _,
		);
	}
	ret(status, value)
}

#[inline]
pub extern "C" fn alloc_dma(base: Option<NonNull<Page>>, size: usize) -> Result {
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
pub extern "C" fn physical_address(base: NonNull<Page>) -> Result {
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

use core::fmt;

#[repr(C)]
pub struct SysLog {
	buffer: [u8; 127],
	pub index: u8,
}

impl SysLog {
	#[optimize(size)]
	fn flush(&mut self) {
		syslog(&self.buffer[..usize::from(self.index)]);
		self.index = 0;
	}
}

impl fmt::Write for SysLog {
	#[optimize(size)]
	fn write_str(&mut self, s: &str) -> fmt::Result {
		for c in s.bytes() {
			if c == b'\n' {
				//|| usize::from(self.index) >= self.buffer.len() {
				self.flush();
			}
			if c != b'\n' {
				self.buffer[usize::from(self.index)] = c;
				self.index += 1;
			}
		}
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
			use $crate::syscall::SysLog;
			let _ = write!(SysLog::default(), $($arg)*);
		}
	};
}

fn ret(status: usize, value: usize) -> Result {
	match NonZeroUsize::new(status) {
		None => Ok(value),
		Some(status) => Err((status, value)),
	}
}
