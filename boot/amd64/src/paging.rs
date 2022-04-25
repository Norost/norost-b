use crate::cpuid;
use crate::mtrr;
use core::fmt;
use core::mem::MaybeUninit;

#[repr(C)]
#[repr(align(4096))]
pub struct Page(MaybeUninit<[u8; 4096]>);

impl Page {
	pub fn zeroed() -> Self {
		Self(MaybeUninit::zeroed())
	}
}

extern "C" {
	static boot_bottom: usize;
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
	pub unsafe fn identity_map<F>(
		&mut self,
		mut page_alloc: F,
		memory_top: u64,
		cpuid: &cpuid::Features,
	) where
		F: FnMut() -> *mut Page,
	{
		debug_assert!(!self.0[0].present());

		assert!(cpuid.mtrr(), "no MTRRs available");

		// Identity-map the bootloader init section, which is located at the start and less than
		// 4KB.
		// 4KB pages are used to avoid undefined behaviour as 2MB/1GB pages can overlap regions
		// with different memory types.
		let init = &boot_bottom as *const _ as usize;
		let pdp = &mut *page_alloc().cast::<DirectoryPointers>();
		let pd = &mut *page_alloc().cast::<Directory>();
		let pt = &mut *page_alloc().cast::<Table>();
		pt.0[(init >> 12) & 0x1ff]
			.set(init.try_into().unwrap(), true, false, true)
			.unwrap();
		pd.0[(init >> 21) & 0x1ff].set(pt);
		pdp.0[(init >> 30) & 0x1ff].set(pd);
		self.0[0].set(pdp);

		let use_1gb = cpuid.pdpe1gb();

		// Identity map the first 64T of available physical memory to 0xffff_c000_0000_0000

		let mtrrs = mtrr::AllRanges::new();

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

				let map_2mb = |pd: &mut Directory, from, page_alloc: &mut F| {
					// Attempt to map as series of 2MBs
					for m in from..512 {
						let addr = addr | u64::try_from(m).unwrap() << 21;
						if addr > memory_top {
							break;
						}
						// Split the page in pieces unconditionally since the first 1MB is a
						// mishmash of memory types no matter what (don't even bother with
						// fixed MTRRs).
						if mtrrs.intersects_2mb(addr) || (t, g, m) == (384, 0, 0) {
							let pt = &mut *page_alloc().cast::<Table>();
							for k in 0..512 {
								let addr = addr | u64::try_from(k).unwrap() << 12;
								if addr > memory_top {
									break;
								}
								pt.0[k].set(addr, true, true, false).unwrap();
							}
							pd.0[m].set(pt);
						} else {
							pd.0[m].set_mega(addr, true, true, false).unwrap();
						}
					}
				};

				// Ditto about first 1MB
				if use_1gb && (t, g) != (384, 0) {
					// Attempt to map whole 1G
					if let Some(m) = mtrrs.intersects_1gb(addr) {
						let pd = &mut *page_alloc().cast::<Directory>();
						// Map all unaffected 2MB frames with hugepages.
						for m in 0..m {
							let addr = addr | u64::try_from(m).unwrap() << 21;
							if addr > memory_top {
								break;
							}
							pd.0[m].set_mega(addr, true, true, false).unwrap();
						}
						// Map affected 2MB page as 4K pages
						let pt = &mut *page_alloc().cast::<Table>();
						for k in 0..512 {
							let addr = addr | u64::try_from(k).unwrap() << 12;
							if addr > memory_top {
								break;
							}
							pt.0[k].set(addr, true, true, false).unwrap();
						}
						pd.0[m].set(pt);
						// Do regular scan on remaining pages
						map_2mb(pd, m + 1, &mut page_alloc);
						pdp.0[g].set(pd);
					} else {
						pdp.0[g].set_giga(addr, true, true, false).unwrap();
					}
				} else {
					let pd = &mut *page_alloc().cast::<Directory>();
					map_2mb(pd, 0, &mut page_alloc);
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

impl fmt::Debug for AddError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(match self {
			AddError::BadRWXFlags => "bad RWX flags",
			AddError::BadAlignment => "bad alignment",
			AddError::Occupied => "address occupied",
			AddError::LowerHalf => "attempt to map lower half",
		})
	}
}

#[derive(Clone, Copy, Debug)]
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
