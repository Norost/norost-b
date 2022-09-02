#[cfg(feature = "driver-vga")]
use crate::driver::vga;
use {
	crate::{
		driver::uart,
		object_table::{Error, Object, Root, Ticket},
	},
	alloc::sync::Arc,
	core::fmt::{self, Write},
};

struct SystemTable;

impl Object for SystemTable {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		if path == b"log" {
			Ticket::new_complete(Ok(Arc::new(SystemLogRef)))
		} else {
			Ticket::new_complete(Err(Error::DoesNotExist))
		}
	}
}

struct SystemLogRef;

impl Object for SystemLogRef {
	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<u64> {
		// TODO make write non-blocking.
		// FIXME avoid panic
		write!(SystemLog::new(), "{}", crate::util::ByteStr::new(data)).unwrap();
		Ticket::new_complete(Ok(data.len().try_into().unwrap()))
	}
}

pub fn post_init(root: &Root) {
	let table = Arc::new(SystemTable) as Arc<dyn Object>;
	root.add(*b"system", Arc::downgrade(&table));
	let _ = Arc::into_raw(table); // Intentionally leak the table.
}

pub struct SystemLog {}

impl SystemLog {
	pub fn new() -> Self {
		Self {}
	}
}

impl fmt::Write for SystemLog {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		uart::get(0).write_str(s)?;
		#[cfg(feature = "driver-vga")]
		vga::TEXT.auto_lock().write_str(s)?;
		Ok(())
	}
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
			writeln!($crate::log::SystemLog::new(), $($args)*).unwrap();
		}
	}};
	($($args:tt)*) => {{
		#[cfg(feature = "debug")]
		{
			#[allow(unused_imports)]
			use core::fmt::Write;
			writeln!($crate::log::SystemLog::new(), $($args)*).unwrap();
		}
	}}
}

#[macro_export]
macro_rules! info {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::log::SystemLog::new(), $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! warn {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::log::SystemLog::new(), $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! error {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::log::SystemLog::new(), $($args)*).unwrap();
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
