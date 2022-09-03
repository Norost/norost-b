#[cfg(feature = "driver-vga")]
use crate::driver::vga;
use {
	crate::{
		driver::uart,
		object_table::{Error, Object, Root, Ticket, TicketWaker},
		sync::SpinLock,
	},
	alloc::{boxed::Box, sync::Arc, vec::Vec},
	core::{
		cell::SyncUnsafeCell,
		fmt::{self, Write},
	},
};

/// The size of the kernel log in bytes.
const SIZE: usize = 1 << 16;

/// The maximum size of a single message.
/// Used to prevent the spinlock from being held for an excessive amount of time.
const MSG_SIZE: usize = 1 << 8;

/// The global kernel log buffer.
static LOG: SpinLock<Log> = SpinLock::new(Log { head: 0, readers: Vec::new() });

/// The actual buffer
///
/// This is stored separately so the .data section doesn't blow up.
static mut BUF: [u8; SIZE] = [0; SIZE];

/// # Message format
///
/// * u64 little-endian timestamp in nanoseconds.
/// * N bytes of arbitrary data.
struct Log {
	readers: Vec<Arc<Reader>>,
	head: usize,
}

/// Append a message to the log.
fn append(msg: &[u8]) -> usize {
	let msg = &msg[..msg.len().min(MSG_SIZE)];

	let mut log = LOG.auto_lock();

	let pre = if SIZE - log.head < msg.len() {
		SIZE - log.head
	} else {
		msg.len()
	};
	let post = msg.len() - pre;
	let h = log.head;
	// SAFETY: we hold the LOG lock
	unsafe {
		BUF[h..][..pre].copy_from_slice(&msg[..pre]);
		BUF[..post].copy_from_slice(&msg[pre..]);
	}

	for r in log.readers.iter_mut() {
		// SAFETY: we hold the LOG lock
		unsafe {
			(*r.wake.get())
				.take()
				.map(|w| w.isr_complete(Ok(msg.into())));
			*r.tail.get() = (*r.tail.get()).wrapping_add(msg.len());
		}
	}

	log.head = log.head.wrapping_add(msg.len());

	msg.len()
}

/// # Safety
///
/// The `LOG` lock must be held before accessing any members.
struct Reader {
	// Only allow one reader at any time to reduce complexity.
	wake: SyncUnsafeCell<Option<TicketWaker<Box<[u8]>>>>,
	tail: SyncUnsafeCell<usize>,
	/// Whether to block or not when no data is available.
	///
	/// Non-blocking mode is useful when copying the log to a file.
	blocking: bool,
}

impl Object for Reader {
	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		let log = LOG.lock();
		// SAFETY: we hold the LOG lock
		unsafe {
			let mut t = *self.tail.get();
			if t != log.head {
				// In case the reader is lagging behind a lot.
				if log.head.wrapping_sub(t) > SIZE {
					t = log.head.wrapping_sub(SIZE);
				}
				let mut l = log.head.wrapping_sub(t).min(length);
				let mut v = Vec::with_capacity(l);
				let b = &BUF[t % SIZE..];
				let b = &b[..b.len().min(l)];
				v.extend_from_slice(b);
				l -= b.len();
				v.extend_from_slice(&BUF[..l]);
				*self.tail.get() = t.wrapping_add(v.len());
				Ticket::new_complete(Ok(b.into()))
			} else if self.blocking {
				let (t, w) = Ticket::new();
				(*self.wake.get())
					.replace(w)
					.map(|w| w.isr_complete(Err(Error::Cancelled)));
				t
			} else {
				Ok([].into()).into()
			}
		}
	}
}

pub struct Writer;

impl Object for Writer {
	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<u64> {
		(write(data) as u64).into()
	}
}

impl Write for Writer {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		write(s.as_ref());
		Ok(())
	}
}

fn write(data: &[u8]) -> usize {
	if data.is_empty() {
		return 0;
	}
	let l = append(data);
	let _ = write!(uart::get(0), "{}", crate::util::ByteStr::new(&data[..l]));
	#[cfg(feature = "driver-vga")]
	let _ = write!(
		vga::TEXT.auto_lock(),
		"{}",
		crate::util::ByteStr::new(&data[..l])
	);
	l
}

struct Table;

impl Object for Table {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(match path {
			b"stream" => Ok(Arc::new(Reader {
				wake: None.into(),
				tail: 0usize.into(),
				blocking: true,
			})),
			b"read" => Ok(Arc::new(Reader {
				wake: None.into(),
				tail: 0usize.into(),
				blocking: false,
			})),
			b"write" => Ok(Arc::new(Writer)),
			_ => Err(Error::DoesNotExist),
		})
	}
}

pub fn post_init(root: &Root) {
	let table = Arc::new(Table) as Arc<dyn Object>;
	root.add(*b"syslog", Arc::downgrade(&table));
	let _ = Arc::into_raw(table); // Intentionally leak the table.
}

pub struct EmergencyWriter;

impl fmt::Write for EmergencyWriter {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		uart::EmergencyWriter.write_str(s)?;
		#[cfg(feature = "driver-vga")]
		vga::EmergencyWriter.write_str(s)?;
		Ok(())
	}
}

#[macro_export]
macro_rules! debug {
	(syscall $($args:tt)*) => {{
		#[cfg(feature = "debug-syscall")]
		{
			#[allow(unused_imports)]
			use core::fmt::Write;
			writeln!($crate::log::Writer, $($args)*).unwrap();
		}
	}};
	($($args:tt)*) => {{
		#[cfg(feature = "debug")]
		{
			#[allow(unused_imports)]
			use core::fmt::Write;
			writeln!($crate::log::Writer, $($args)*).unwrap();
		}
	}}
}

#[macro_export]
macro_rules! info {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::log::Writer, $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! warn {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::log::Writer, $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! error {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::log::Writer, $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! fatal {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::log::EmergencyWriter, $($args)*).unwrap();
	}}
}

// Shamelessly copied from stdlib.
#[macro_export]
macro_rules! dbg {
    () => {
        $crate::fatal!("[{}:{}]", file!(), line!());
    };
    ($val:expr $(,)?) => {
        // Use of `match` here is intentional because it affects the lifetimes
        // of temporaries - https://stackoverflow.com/a/48732525/1063961
        match $val {
            tmp => {
                $crate::fatal!("[{}:{}] {} = {:#?}",
                    file!(), line!(), stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbg!($val)),+,)
    };
}
