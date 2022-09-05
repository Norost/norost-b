// Shamelessly copied from stdlib.
#[macro_export]
macro_rules! dbg {
    () => {{
        let _ = $crate::io::stderr().map(|o| writeln!(o, "[{}:{}]", file!(), line!()));
    }};
    ($val:expr $(,)?) => {
        // Use of `match` here is intentional because it affects the lifetimes
        // of temporaries - https://stackoverflow.com/a/48732525/1063961
        match $val {
            tmp => {
				let _ = $crate::io::stderr().map(|o| {
					writeln!(o, "[{}:{}] {} = {:#?}", file!(), line!(), stringify!($val), &tmp)
				});
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbg!($val)),+,)
    };
}

#[macro_export]
macro_rules! print {
    ($fmt:expr $(,)?) => {{
        $crate::io::_print_str($fmt);
    }};
    ($fmt:expr, $($arg:tt)*) => {{
        $crate::io::_print(format_args!($fmt, $($arg)*));
    }};
}

#[macro_export]
macro_rules! eprint {
    ($fmt:expr $(,)?) => {{
        $crate::io::_eprint_str($fmt);
    }};
    ($fmt:expr, $($arg:tt)*) => {{
        $crate::io::_eprint(format_args!($fmt, $($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
	() => {{
        $crate::io::_print_str("\n");
    }};
    ($fmt:expr $(,)?) => {{
        $crate::io::_print_str(concat!($fmt, "\n"));
    }};
    ($fmt:expr, $($arg:tt)*) => {{
        $crate::io::_print(format_args!(concat!($fmt, "\n"), $($arg)*));
    }};
}

#[macro_export]
macro_rules! eprintln {
	() => {{
        $crate::io::_eprint_str("\n");
    }};
    ($fmt:expr $(,)?) => {{
        $crate::io::_eprint_str(concat!($fmt, "\n"));
    }};
    ($fmt:expr, $($arg:tt)*) => {{
        $crate::io::_eprint(format_args!(concat!($fmt, "\n"), $($arg)*));
    }};
}
