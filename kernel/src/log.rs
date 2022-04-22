pub unsafe fn init() {}

#[macro_export]
macro_rules! debug {
	($($args:tt)*) => {
		#[cfg(debug_assertions)]
		{
			#[allow(unused_imports)]
			use core::fmt::Write;
			writeln!($crate::driver::uart::get(0), $($args)*).unwrap();
		}
		#[cfg(not(debug_assertions))]
		{}
	}
}

#[macro_export]
macro_rules! info {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::driver::uart::get(0), $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! warn {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::driver::uart::get(0), $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! error {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::driver::uart::get(0), $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! fatal {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		writeln!($crate::driver::uart::get(0), $($args)*).unwrap();
	}}
}

// Shamelessly copied from stdlib.
#[macro_export]
macro_rules! dbg {
    () => {
        $crate::info!("[{}:{}]", file!(), line!());
    };
    ($val:expr $(,)?) => {
        // Use of `match` here is intentional because it affects the lifetimes
        // of temporaries - https://stackoverflow.com/a/48732525/1063961
        match $val {
            tmp => {
                $crate::info!("[{}:{}] {} = {:#?}",
                    file!(), line!(), stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbg!($val)),+,)
    };
}
