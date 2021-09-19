use crate::sync::SpinLock;
use crate::driver::vga::text::Text;

pub static __VGA: SpinLock<Text> = SpinLock::new(Text::new());

#[macro_export]
macro_rules! debug {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		let mut vga = $crate::log::__VGA.lock();
		vga.set_colors(0x7, 0);
		writeln!(vga, $($args)*).unwrap();
	}}
}

#[macro_export]
macro_rules! fatal {
	($($args:tt)*) => {{
		#[allow(unused_imports)]
		use core::fmt::Write;
		let mut vga = $crate::log::__VGA.lock();
		vga.set_colors(0xc, 0);
		writeln!(vga, $($args)*).unwrap();
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
