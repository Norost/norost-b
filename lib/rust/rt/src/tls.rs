use core::mem;
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

const ENTRIES: usize = 128;

#[cfg(not(feature = "rustc-dep-of-std"))]
extern crate alloc;

use alloc::boxed::Box;

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
		let mut slf = Self {
			bits: [const { AtomicUsize::new(0) }; ENTRIES / Self::BITS_PER_ENTRY],
		};
		// Reservations:
		// 0) QUEUE_KEY in io.rs
		slf.bits[0] = AtomicUsize::new(1);
		slf
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

#[derive(Clone, Copy)]
pub struct Key(pub usize);

#[repr(C)]
struct Entry {
	data: *mut (),
}

// See lib.rs
//
// We use a function because "must have type `*const T` or `*mut T` due to `#[linkage]` attribute"
//
// TODO this doesn't get inlined :(
#[linkage = "weak"]
#[export_name = "__rt_tls_allocated"]
fn allocated() -> &'static Bitset {
	static ALLOCATED: Bitset = Bitset::new();
	&ALLOCATED
}

// See lib.rs
#[linkage = "weak"]
#[export_name = "__rt_tls_destructors"]
fn destructors() -> &'static [AtomicPtr<()>; ENTRIES] {
	static DESTRUCTORS: [AtomicPtr<()>; ENTRIES] = {
		let mut d = [const { AtomicPtr::new(ptr::null_mut()) }; ENTRIES];
		d[0] = AtomicPtr::new(crate::io::queue_dtor as _);
		d
	};
	&DESTRUCTORS
}

/// Create & initialize TLS storage for a new thread.
#[must_use = "this must be passed to the new thread"]
pub(crate) fn create_for_thread(
	init: impl Iterator<Item = (Key, *mut ())>,
) -> crate::io::Result<*mut ()> {
	// TODO allocation may fail.
	let mut entries = Box::<[Entry]>::new_zeroed_slice(ENTRIES);
	for (k, data) in init {
		entries[k.0].write(Entry { data });
	}
	Ok(Box::into_raw(entries).cast())
}

/// Initialize TLS storage for the current thread.
///
/// # Safety
///
/// This function may only be called once before `deinit_thread`.
///
/// # Note
///
/// [`crate::thread::init`] should be preferred.
pub(crate) unsafe fn init_thread(ptr: *mut ()) {
	unsafe {
		super::set_tls(ptr);
	}
}

/// Destroy the TLS storage for a single thread. This also runs the destructor
/// on any stored values.
///
/// # Safety
///
/// This function may only be called once after `init_thread`.
///
/// The given pointer must come from [`create_for_thread`].
///
/// # Note
///
/// [`crate::thread::deinit`] should be preferred.
pub(crate) unsafe fn deinit_thread() {
	unsafe {
		let storage = super::get_tls().cast::<Entry>();
		let dtors = destructors();
		for key in (0..ENTRIES).map(Key) {
			let val = get(key);
			if !val.is_null() {
				let dtor = dtors[key.0].load(Ordering::Relaxed);
				if !dtor.is_null() {
					mem::transmute::<_, unsafe extern "C" fn(*mut ())>(dtor)(val);
				}
			}
		}
		Box::from_raw(ptr::slice_from_raw_parts_mut(storage, ENTRIES));
	}
}

/// Initialize the runtime.
///
/// # Safety
///
/// This function may only be called once.
pub(crate) unsafe fn init() {
	let entries = create_for_thread([].into_iter()).unwrap_or_else(|_| {
		// We can't do anything
		core::intrinsics::abort()
	});
	unsafe {
		init_thread(entries);
	}
}

/// Allocate a key.
#[inline]
pub fn allocate(destructor: Option<unsafe extern "C" fn(*mut ())>) -> Result<Key, Full> {
	let key = allocated().allocate()?;
	destructors()[key].store(
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
	allocated().clear(key.0);
}

/// Set data associated with a key.
///
/// # Safety
///
/// [`init_thread`] must have been called for this thread.
///
/// Only keys returned from [`allocate`] may be used.
#[inline]
pub unsafe fn set(key: Key, data: *mut ()) {
	unsafe {
		super::write_tls_offset(key.0, data as usize);
	}
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
pub unsafe fn get(key: Key) -> *mut () {
	unsafe { super::read_tls_offset(key.0) as *mut _ }
}
