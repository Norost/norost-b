use crate::sync::SpinLock;
use crate::driver::uart::x86::UART;

pub static __LOG: SpinLock<Option<UART>> = SpinLock::new(None);

pub fn init() {
	*__LOG.lock() = unsafe { Some(UART::new(0x3f8)) };
}

#[macro_export]
macro_rules! debug {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		let mut log = $crate::log::__LOG.lock();
		writeln!(log.as_mut().unwrap(), $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! info {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		let mut log = $crate::log::__LOG.lock();
		writeln!(log.as_mut().unwrap(), $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! fatal {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		let mut log = $crate::log::__LOG.lock();
		writeln!(log.as_mut().unwrap(), $($args)*).unwrap();
	}}
}

// Shamelessly copied from stdlib.
#[macro_export]
macro_rules! dbg {
    () => {
        $crate::debug!("[{}:{}]", file!(), line!());
    };
    ($val:expr $(,)?) => {
        // Use of `match` here is intentional because it affects the lifetimes
        // of temporaries - https://stackoverflow.com/a/48732525/1063961
        match $val {
            tmp => {
                $crate::debug!("[{}:{}] {} = {:#?}",
                    file!(), line!(), stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbg!($val)),+,)
    };
}
