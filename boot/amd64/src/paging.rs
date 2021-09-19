use core::convert::TryFrom;
use core::mem::MaybeUninit;

#[repr(C)]
#[repr(align(4096))]
pub struct Page(MaybeUninit<[u8; 4096]>);

impl Page {
	pub fn zeroed() -> Self {
		Self(MaybeUninit::zeroed())
	}
}

#[repr(C)]
#[repr(align(4096))]
pub struct PML4([PML4Entry; 512]);

#[repr(transparent)]
struct PML4Entry(u64);

#[repr(C)]
#[repr(align(4096))]
struct DirectoryPointers([DirectoryPointersEntry; 512]);

#[repr(transparent)]
struct DirectoryPointersEntry(u64);

#[repr(C)]
#[repr(align(4096))]
struct Directory([DirectoryEntry; 512]);

#[repr(transparent)]
struct DirectoryEntry(u64);

#[repr(C)]
#[repr(align(4096))]
struct Table([TableEntry; 512]);

#[repr(transparent)]
struct TableEntry(u64);

impl PML4 {
	pub fn new() -> Self {
		const V: PML4Entry = PML4Entry::new();
		Self([V; 512])
	}

	/// `memory_top` is *inclusive*.
	///
	/// # Safety
	///
	/// May only be called once.
	pub unsafe fn identity_map<F>(&mut self, mut page_alloc: F, memory_top: u64)
	where
		F: FnMut() -> *mut Page,
	{
		debug_assert!(!self.0[0].present());

		// FIXME account for different memory types
		const PDPE1GB: u64 = 1 << (32 + 26); // stolen from Linux (arch/x86/include/asm/cpufeatures.h)
		let use_1gb = {
			let (ecx, edx): (u32, u32);
			asm!("cpuid", in("eax") 0x8000_0001u32, out("ecx") ecx, out("edx") edx, options(att_syntax));
			let features = (u64::from(edx) << 32) | u64::from(ecx);
			features & PDPE1GB > 0
		};

		// Identity-map the first 1G.

		let pdp = &mut *page_alloc().cast::<DirectoryPointers>();
		if use_1gb {
			pdp.0[0].set_giga(0, true, true, true).unwrap_or_else(|_| unreachable!());
		} else {
			let pd = &mut *page_alloc().cast::<Directory>();
			for m in 0usize..512 {
				let addr = u64::try_from(m).unwrap() << 21;
				pd.0[m].set_mega(addr, true, true, true).unwrap_or_else(|_| unreachable!());
			}
			pdp.0[0].set(pd);
		}
		self.0[0].set(pdp);

		// Identity map the first 64T of available physical memory to 0xffff_c000_0000_0000
		// FIXME account for different memory types

		assert!(memory_top < 1 << 46);
		for t in 384..512 {

			let addr = u64::try_from(t - 384).unwrap() << 39;
			if addr > memory_top {
				break;
			}
			let pdp = &mut *page_alloc().cast::<DirectoryPointers>();

			for g in 0..512 {
				let addr = addr | u64::try_from(g).unwrap() << 30;
				if addr > memory_top {
					break;
				}

				if use_1gb {
					pdp.0[g].set_giga(addr, true, true, false).unwrap_or_else(|_| unreachable!());
				} else {
					let pd = &mut *page_alloc().cast::<Directory>();
					for m in 0..512 {
						let addr = addr | u64::try_from(m).unwrap() << 21;
						if addr > memory_top {
							break;
						}
						pd.0[m].set_mega(addr, true, true, false).unwrap_or_else(|_| unreachable!());
					}
					pdp.0[g].set(pd);
				}
			}
			self.0[t].set(pdp);
		}
	}

	pub fn add<F>(
		&mut self,
		virt: u64,
		phys: u64,
		read: bool,
		write: bool,
		execute: bool,
		mut page_alloc: F,
	) -> Result<(), AddError>
	where
		F: FnMut() -> *mut Page,
	{
		(virt & 0xfff == 0)
			.then(|| ())
			.ok_or(AddError::BadAlignment)?;
		(phys & 0xfff == 0)
			.then(|| ())
			.ok_or(AddError::BadAlignment)?;
		// Ensure kernel is placed entirely in higher half
		(virt & (1 << 63) > 0)
			.then(|| ())
			.ok_or(AddError::LowerHalf)?;

		// PML4
		let tbl = &mut self.0[usize::try_from((virt >> 39) & 0x1ff).unwrap()];
		let tbl = match tbl.get() {
			Some(tbl) => tbl,
			None => unsafe {
				let p = &mut *page_alloc().cast::<DirectoryPointers>();
				tbl.set(p);
				p
			},
		};

		// PDP
		let tbl = &mut tbl.0[usize::try_from((virt >> 30) & 0x1ff).unwrap()];
		let tbl = match tbl.get() {
			Some(tbl) => tbl,
			None => unsafe {
				let p = &mut *page_alloc().cast::<Directory>();
				tbl.set(p);
				p
			},
		};

		// PD
		let tbl = &mut tbl.0[usize::try_from((virt >> 21) & 0x1ff).unwrap()];
		let tbl = match tbl.get() {
			Some(tbl) => tbl,
			None => unsafe {
				let p = &mut *page_alloc().cast::<Table>();
				tbl.set(p);
				p
			},
		};

		// PT
		let tbl = &mut tbl.0[usize::try_from((virt >> 12) & 0x1ff).unwrap()];
		match tbl.get() {
			Some(_) => Err(AddError::Occupied)?,
			None => tbl.set(phys, read, write, execute)?,
		};

		Ok(())
	}

	pub unsafe fn activate(&self) {
		asm!("
			# Enable PAE
			movl	%cr4, %eax
			orl		$0x20, %eax
			movl	%eax, %cr4
			# Set PML4
			movl	{0}, %cr3
			# Enable long mode
			movl	$0xc0000080, %ecx	# IA32_EFER
			rdmsr
			orl		$0x100, %eax		# Enable long mode
			wrmsr
			# Enable paging
			movl	%cr0, %eax
			orl		$0x80000000, %eax
			movl	%eax, %cr0
		", in(reg) self as *const _, out("eax") _, out("ecx") _, out("edx") _, options(att_syntax));
	}
}

impl PML4Entry {
	const fn new() -> Self {
		Self(0)
	}

	fn present(&self) -> bool {
		self.0 & 1 > 0
	}

	/// # Safety
	///
	/// `pdp` must be properly initialized.
	unsafe fn set(&mut self, pdp: *mut DirectoryPointers) {
		self.0 = pdp as u64 | 1;
	}

	fn get(&mut self) -> Option<&mut DirectoryPointers> {
		self.present()
			.then(|| unsafe { &mut *((self.0 as usize & !1) as *mut _) })
	}
}

impl DirectoryPointersEntry {
	fn present(&self) -> bool {
		self.0 & 1 > 0
	}

	/// # Safety
	///
	/// `pd` must be properly initialized.
	unsafe fn set(&mut self, pd: *mut Directory) {
		self.0 = pd as u64 | 1;
	}

	fn set_giga(&mut self, page: u64, r: bool, w: bool, x: bool) -> Result<(), SetError> {
		(page & ((1 << 21) - 1) == 0)
			.then(|| ())
			.ok_or(SetError::BadAlignment)?;
		self.0 = page | (1 << 7) | rwx_flags(r, w, x)? | 1;
		Ok(())
	}

	fn is_giga(&self) -> bool {
		self.0 & (1 << 7) > 0
	}

	fn get(&mut self) -> Option<&mut Directory> {
		(self.present() && !self.is_giga())
			.then(|| unsafe { &mut *((self.0 as usize & !1) as *mut _) })
	}
}

impl DirectoryEntry {
	fn present(&self) -> bool {
		self.0 & 1 > 0
	}

	/// # Safety
	///
	/// `pd` must be properly initialized.
	unsafe fn set(&mut self, pt: *mut Table) {
		self.0 = pt as u64 | 1;
	}

	fn set_mega(&mut self, page: u64, r: bool, w: bool, x: bool) -> Result<(), SetError> {
		(page & ((1 << 21) - 1) == 0)
			.then(|| ())
			.ok_or(SetError::BadAlignment)?;
		self.0 = page | (1 << 7) | rwx_flags(r, w, x)? | 1;
		Ok(())
	}

	fn is_mega(&self) -> bool {
		self.0 & (1 << 7) > 0
	}

	fn get(&mut self) -> Option<&mut Table> {
		(self.present() && !self.is_mega())
			.then(|| unsafe { &mut *((self.0 as usize & !1) as *mut _) })
	}
}

impl TableEntry {
	fn present(&self) -> bool {
		self.0 & 1 > 0
	}

	fn set(&mut self, page: u64, r: bool, w: bool, x: bool) -> Result<(), SetError> {
		(page & ((1 << 12) - 1) == 0)
			.then(|| ())
			.ok_or(SetError::BadAlignment)?;
		self.0 = page | rwx_flags(r, w, x)? | 1;
		Ok(())
	}

	fn get(&mut self) -> Option<u64> {
		self.present().then(|| self.0 & !0xfff)
	}
}

#[derive(Clone, Copy)]
pub enum AddError {
	BadRWXFlags,
	BadAlignment,
	Occupied,
	LowerHalf,
}

impl From<SetError> for AddError {
	fn from(err: SetError) -> Self {
		match err {
			SetError::BadRWXFlags => Self::BadRWXFlags,
			SetError::BadAlignment => Self::BadAlignment,
		}
	}
}

impl From<AddError> for &'static str {
	fn from(err: AddError) -> Self {
		match err {
			AddError::BadRWXFlags => <&'static str as From<_>>::from(BadRWXFlags),
			AddError::BadAlignment => "bad alignment",
			AddError::Occupied => "address occupied",
			AddError::LowerHalf => "attempt to map lower half",
		}
	}
}

#[derive(Clone, Copy)]
pub enum SetError {
	BadRWXFlags,
	BadAlignment,
}

impl From<BadRWXFlags> for SetError {
	fn from(_: BadRWXFlags) -> Self {
		Self::BadRWXFlags
	}
}

#[derive(Clone, Copy)]
struct BadRWXFlags;

impl From<BadRWXFlags> for &'static str {
	fn from(_: BadRWXFlags) -> Self {
		"bad RWX flags"
	}
}

fn rwx_flags(r: bool, w: bool, x: bool) -> Result<u64, BadRWXFlags> {
	match (r, w, x) {
		(true, true, true) => Ok(1 << 1),
		(false, true, true) => Err(BadRWXFlags),
		(true, false, true) => Ok(0),
		(false, false, true) => Ok(0),
		(true, true, false) => Ok(1 << 1),
		(false, true, false) => Err(BadRWXFlags),
		(true, false, false) => Ok(0),
		(false, false, false) => Err(BadRWXFlags),
	}
}
