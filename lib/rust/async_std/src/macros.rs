// Yoinked from stdlib

#[macro_export]
macro_rules! print {
	($($arg:tt)*) => {
		$crate::io::__print(format_args!($($arg)*))
	};
}

#[macro_export]
macro_rules! println {
	() => {
		$crate::print!("\n")
	};
	($($arg:tt)*) => {
		(async {
			$crate::io::__print(format_args!($($arg)*)).await;
			$crate::print!("\n").await;
		})
	};
}

#[macro_export]
macro_rules! eprint {
	($($arg:tt)*) => {
		$crate::io::__eprint(format_args!($($arg)*))
	};
}

#[macro_export]
macro_rules! eprintln {
	() => {
		$crate::eprint!("\n")
	};
	($($arg:tt)*) => {
		(async {
			$crate::io::__eprint(format_args!($($arg)*)).await;
			$crate::print!("\n").await;
		})
	};
}

#[macro_export]
macro_rules! dbg {
    () => {
		$crate::eprintln!("[{}:{}]", file!(), line!()).await
    };
    ($val:expr $(,)?) => {
        // Use of `match` here is intentional because it affects the lifetimes
        // of temporaries - https://stackoverflow.com/a/48732525/1063961
        match $val {
            tmp => {
				$crate::eprintln!("[{}:{}] {} = {:#?}", file!(), line!(), stringify!($val), &tmp).await;
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbg!($val)),+,)
    };
}
