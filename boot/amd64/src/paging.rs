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

	/// # Safety
	///
	/// May only be called once.
	pub unsafe fn identity_map<F>(&mut self, mut page_alloc: F)
	where
		F: FnMut() -> *mut Page,
	{
		assert!(!self.0[0].present());

		// Map the first 2M
		//
		// 1G hugepages can be used on some processors, but 2M has wider compatibility and is
		// plenty.

		let pd = &mut *page_alloc().cast::<Directory>();
		pd.0[0].set_mega(0, true, true, true);

		let pdp = &mut *page_alloc().cast::<DirectoryPointers>();
		pdp.0[0].set(pd);

		self.0[0].set(pdp);
	}

	pub fn add<F>(
		&mut self,
		virt: u64,
		phys: u64,
		read: bool,
		write: bool,
		execute: bool,
		mut page_alloc: F,
	) where
		F: FnMut() -> *mut Page,
	{
		assert_eq!(virt & 0xfff, 0);
		assert_eq!(phys & 0xfff, 0);
		// Ensure kernel is placed entirely in higher half
		assert!(virt & (1 << 63) > 0);

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
		let tbl = match tbl.get() {
			Some(tbl) => panic!("occupied"),
			None => unsafe {
				tbl.set(phys, read, write, execute);
			},
		};
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

	fn set_mega(&mut self, page: u64, r: bool, w: bool, x: bool) {
		assert_eq!(page & ((1 << 21) - 1), 0);
		self.0 = page | (1 << 7) | rwx_flags(r, w, x) | 1;
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

	fn set(&mut self, page: u64, r: bool, w: bool, x: bool) {
		assert_eq!(page & ((1 << 12) - 1), 0);
		self.0 = page | rwx_flags(r, w, x) | 1;
	}

	fn get(&mut self) -> Option<u64> {
		self.present().then(|| self.0 & !0xfff)
	}
}

fn rwx_flags(r: bool, w: bool, x: bool) -> u64 {
	match (r, w, x) {
		(true, true, true) => 1 << 1,
		(false, true, true) => panic!(),
		(true, false, true) => 0,
		(false, false, true) => 0,
		(true, true, false) => 1 << 1,
		(false, true, false) => panic!(),
		(true, false, false) => 0,
		(false, false, false) => panic!(),
	}
}
