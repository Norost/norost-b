use core::mem;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

const ENTRIES: usize = 128;

#[derive(Debug)]
pub struct Full;

/// Structure to keep track of allocated keys.
///
/// Keys are shared between threads, so this is atomic.
struct Bitset {
	bits: [AtomicUsize; ENTRIES / Self::BITS_PER_ENTRY],
}

impl Bitset {
	const BITS_PER_ENTRY: usize = mem::size_of::<AtomicUsize>() * 8;

	const fn new() -> Self {
		Self {
			bits: [const { AtomicUsize::new(0) }; ENTRIES / Self::BITS_PER_ENTRY],
		}
	}

	/// Clear a bit
	fn clear(&self, bit: usize) {
		let (i, bi) = (bit / Self::BITS_PER_ENTRY, bit % Self::BITS_PER_ENTRY);
		let e = &self.bits[i];
		let mut v = e.load(Ordering::Relaxed);
		while let Err(nv) =
			e.compare_exchange(v, v & !(1 << bi), Ordering::Relaxed, Ordering::Relaxed)
		{
			v = nv;
		}
	}

	/// Allocate a clear bit
	fn allocate(&self) -> Result<usize, Full> {
		// Find slot with unused bits
		for (i, e) in self.bits.iter().enumerate() {
			let mut v = e.load(Ordering::Relaxed);
			while v != usize::MAX {
				// Find any unused bit
				let bi = v.trailing_ones() as usize;
				// Set bit
				let nv = v | (1 << bi);
				// Try to update entry. If it fails, something else set it before
				// us. Try again with another bit.
				match e.compare_exchange(v, nv, Ordering::Relaxed, Ordering::Relaxed) {
					Ok(_) => return Ok(i * Self::BITS_PER_ENTRY + bi),
					Err(nv) => v = nv,
				}
			}
		}
		Err(Full)
	}
}

pub struct Key(pub usize);

#[repr(C)]
struct Entry {
	data: *mut u8,
}

static ALLOCATED: Bitset = Bitset::new();
static DESTRUCTORS: [AtomicPtr<()>; ENTRIES] = [const { AtomicPtr::new(ptr::null_mut()) }; ENTRIES];

/// Initialize TLS storage for a single thread.
///
/// # Safety
///
/// This function may only be called once before `deinit_thread`.
#[inline]
pub unsafe fn init_thread(alloc: impl FnOnce(usize) -> NonNull<[u8]>) {
	let ptr = alloc(ENTRIES * mem::size_of::<Entry>());
	super::set_tls(ptr.as_ptr().as_mut_ptr().cast());
}

/// Initialize TLS storage for a single thread.
///
/// # Safety
///
/// This function may only be called once after `init_thread`.
#[inline]
pub unsafe fn deinit_thread(dealloc: impl FnOnce(NonNull<[u8]>)) {
	let ptr = NonNull::new(super::get_tls()).unwrap();
	dealloc(NonNull::slice_from_raw_parts(
		ptr.cast(),
		ENTRIES * mem::size_of::<Entry>(),
	));
}

/// Allocate a key.
#[inline]
pub fn allocate(destructor: Option<unsafe extern "C" fn(*mut u8)>) -> Result<Key, Full> {
	let key = ALLOCATED.allocate()?;
	DESTRUCTORS[key].store(
		destructor.map_or(ptr::null_mut(), |f| f as *mut ()),
		Ordering::Relaxed,
	);
	Ok(Key(key))
}

/// Free a key.
///
/// # Safety
///
/// The key may not be reused after this call.
#[inline]
pub unsafe fn free(key: Key) {
	ALLOCATED.clear(key.0);
}

/// Set data associated with a key.
///
/// # Safety
///
/// [`init_thread`] must have been called for this thread.
///
/// Only keys returned from [`allocate`] may be used.
#[inline]
pub unsafe fn set(key: Key, data: *mut u8) {
	super::write_tls_offset(key.0, data as usize);
}

/// Get data associated with a key.
///
/// # Safety
///
/// [`init_thread`] must have been called for this thread.
///
/// Only keys returned from [`allocate`] may be used.
///
/// The data must be initialized with [`set_data`].
#[inline]
pub unsafe fn get(key: Key) -> *mut u8 {
	super::read_tls_offset(key.0) as *mut _
}
