//! ## Arguments format
//!
//! Strings are prefixed with a two-byte (`u16`) length.
//! They are *not* null-terminated.
//!
//! The passed arguments contains arrays:
//!
//! - Handles, where the key is a string and the value is a 32-bit integer representing a handle
//!   to an object.
//! - "Command" arguments, where the values are strings.
//! - Environment variables, where the keys and values are strings.
//!
//! Each array is prefixed with a two-byte (`u16`) length.

use {
	crate::{
		sync::{Mutex, MutexGuard},
		RefObject,
	},
	alloc::{
		alloc::AllocError,
		borrow::Cow,
		collections::{btree_map, BTreeMap},
	},
	core::{
		fmt,
		ptr::{self, NonNull},
		slice,
		sync::atomic::{AtomicPtr, Ordering},
	},
};

// See note in lib.rs
#[export_name = "__rt_args_handles"]
#[linkage = "weak"]
static HANDLES: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
// See note in lib.rs
#[export_name = "__rt_args_args_and_env"]
#[linkage = "weak"]
static ARGS_AND_ENV: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
// See note in lib.rs
#[export_name = "__rt_args_env"]
#[linkage = "weak"]
static ENV: Mutex<(bool, BTreeMap<Cow<'static, [u8]>, Cow<'static, [u8]>>)> =
	Mutex::new((false, BTreeMap::new()));

pub struct Args {
	count: u16,
	ptr: *const u8,
}

impl Args {
	fn new() -> Self {
		NonNull::new(ARGS_AND_ENV.load(Ordering::Relaxed)).map_or(
			Self { count: 0, ptr: ptr::null() },
			|ptr| {
				let ptr = ptr.cast::<u16>();
				unsafe {
					Self { count: ptr.as_ptr().read_unaligned(), ptr: ptr.as_ptr().add(1).cast() }
				}
			},
		)
	}

	/// This method is used by Rust's standard library as Args is a [`DoubleEndedIterator`] for
	/// some reason.
	#[doc(hidden)]
	pub fn next_back(&mut self) -> Option<&'static [u8]> {
		self.count.checked_sub(1).map(|c| {
			// Very inefficient but w/e, it shouldn't matter.
			let args = Self { count: self.count, ptr: self.ptr };
			self.count = c;
			args.last().unwrap()
		})
	}
}

impl fmt::Debug for Args {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let args = Args { count: self.count, ptr: self.ptr };
		let mut f = f.debug_list();
		for e in args {
			f.entry(&e);
		}
		f.finish()
	}
}

impl Iterator for Args {
	type Item = &'static [u8];

	fn next(&mut self) -> Option<Self::Item> {
		self.count.checked_sub(1).map(|c| {
			self.count = c;
			unsafe {
				let (val, ptr) = get_str(self.ptr);
				self.ptr = ptr;
				val
			}
		})
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		(self.len(), Some(self.len()))
	}
}

impl ExactSizeIterator for Args {
	fn len(&self) -> usize {
		usize::from(self.count)
	}
}

#[derive(Debug)]
pub struct Env {
	inner: btree_map::IntoIter<Cow<'static, [u8]>, Cow<'static, [u8]>>,
}

impl Iterator for Env {
	type Item = (Cow<'static, [u8]>, Cow<'static, [u8]>);

	fn next(&mut self) -> Option<Self::Item> {
		self.inner.next()
	}
}

impl Env {
	fn get_env() -> MutexGuard<'static, (bool, BTreeMap<Cow<'static, [u8]>, Cow<'static, [u8]>>)> {
		let mut map = ENV.lock();
		if !map.0 {
			// A finished args iterator will point to the start of the env variables.
			let mut args = Args::new();
			(&mut args).last();
			// Load all env variables in a map so we can easily modify & remove variables.
			if args.ptr != core::ptr::null() {
				unsafe {
					let ptr = args.ptr.cast::<u16>();
					let count = usize::from(ptr.read_unaligned());
					let mut ptr = ptr.add(1).cast::<u8>();
					for _ in 0..count {
						let (key, p) = get_str(ptr);
						let (val, p) = get_str(p);
						map.1.insert(key.into(), val.into());
						ptr = p;
					}
				}
			}
			map.0 = true;
		}
		map
	}

	fn new() -> Self {
		// "The returned iterator contains a snapshot of the process’s environment variables ..." &
		// "Modifications to environment variables afterwards will not be reflected ..."
		// means we need to clone it, or at least use some kind of CoW.
		Self { inner: Self::get_env().1.clone().into_iter() }
	}

	pub fn get(key: &[u8]) -> Option<Cow<'static, [u8]>> {
		Self::get_env().1.get(key).cloned()
	}

	pub fn try_insert(
		key: Cow<'static, [u8]>,
		value: Cow<'static, [u8]>,
	) -> Result<Option<Cow<'static, [u8]>>, AllocError> {
		// TODO avoid potentially panicking.
		Ok(Self::get_env().1.insert(key.into(), value.into()))
	}

	pub fn remove(key: &[u8]) -> Option<Cow<'static, [u8]>> {
		Self::get_env().1.remove(key)
	}
}

/// # Safety
///
/// Must be called exactly once during runtime initialization.
pub(crate) unsafe fn init(arguments: Option<NonNull<u8>>) {
	let Some(arguments) = arguments else { return };

	HANDLES.store(arguments.as_ptr(), Ordering::Relaxed);

	// Parse handles
	unsafe {
		let mut arguments = arguments.as_ptr() as *const u8;
		let count = arguments.cast::<u16>().read_unaligned();
		arguments = arguments.add(2);
		for _ in 0..count {
			let name;
			(name, arguments) = get_str(arguments);
			let handle = arguments.cast::<u32>().read_unaligned();
			arguments = arguments.wrapping_add(4);
			let globals = crate::globals::GLOBALS.get_ref();
			match name {
				b"in" => globals.stdin_handle.store(handle, Ordering::Relaxed),
				b"out" => globals.stdout_handle.store(handle, Ordering::Relaxed),
				b"err" => globals.stderr_handle.store(handle, Ordering::Relaxed),
				b"file" => globals.file_root_handle.store(handle, Ordering::Relaxed),
				b"net" => globals.net_root_handle.store(handle, Ordering::Relaxed),
				b"process" => globals.process_root_handle.store(handle, Ordering::Relaxed),
				_ => {} // Just ignore.
			}
		}

		// Store pointer for later use
		ARGS_AND_ENV.store(arguments.cast::<u8>() as *mut _, Ordering::Relaxed)
	}
}

unsafe fn get_str<'a>(ptr: *const u8) -> (&'a [u8], *const u8) {
	unsafe {
		let len = usize::from(ptr.cast::<u16>().read_unaligned());
		let ptr = ptr.wrapping_add(2);
		(slice::from_raw_parts(ptr, len), ptr.add(len))
	}
}

/// Iterator over all objects that have been passed to this program.
pub struct Handles {
	ptr: NonNull<u8>,
	count: u16,
}

impl Iterator for Handles {
	type Item = (&'static [u8], RefObject<'static>);

	fn next(&mut self) -> Option<Self::Item> {
		self.count.checked_sub(1).map(|n| unsafe {
			self.count = n;
			let (name, ptr) = get_str(self.ptr.as_ptr());
			let h = ptr.cast::<u32>().read_unaligned();
			self.ptr = NonNull::new_unchecked(ptr.add(4) as _);
			(name, RefObject::from_raw(h))
		})
	}
}

pub fn handles() -> Handles {
	NonNull::new(HANDLES.load(Ordering::Relaxed)).map_or(
		Handles { ptr: NonNull::dangling(), count: 0 },
		|ptr| unsafe {
			Handles {
				ptr: NonNull::new_unchecked(ptr.as_ptr().add(2).cast()),
				count: ptr.as_ptr().cast::<u16>().read_unaligned(),
			}
		},
	)
}

pub fn args() -> Args {
	Args::new()
}

pub fn env() -> Env {
	Env::new()
}

pub fn handle(name: &[u8]) -> Option<RefObject<'static>> {
	handles().find_map(|(n, o)| (n == name).then(|| o))
}
