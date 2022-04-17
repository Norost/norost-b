use std::io::{self, Read, Seek, SeekFrom, Write};

/// A wrapper that will execute a callback any time an I/O event occurs.
pub struct Monitor<T, F: FnMut(Event<'_>)> {
	inner: T,
	callback: F,
}

/// The I/O event that occured.
pub enum Event<'a> {
	Read {
		data: &'a [u8],
		result: &'a io::Result<usize>,
	},
	Write {
		data: &'a [u8],
		result: &'a io::Result<usize>,
	},
	Flush {
		result: &'a io::Result<()>,
	},
	Seek {
		from: SeekFrom,
		result: &'a io::Result<u64>,
	},
}

impl<T, F: FnMut(Event<'_>)> Monitor<T, F> {
	pub fn new(inner: T, callback: F) -> Self {
		Self { inner, callback }
	}
}

impl<T: Read, F: FnMut(Event<'_>)> Read for Monitor<T, F> {
	fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
		let result = self.inner.read(data);
		(self.callback)(Event::Read {
			data,
			result: &result,
		});
		result
	}
}

impl<T: Write, F: FnMut(Event<'_>)> Write for Monitor<T, F> {
	fn write(&mut self, data: &[u8]) -> io::Result<usize> {
		let result = self.inner.write(data);
		(self.callback)(Event::Write {
			data,
			result: &result,
		});
		result
	}

	fn flush(&mut self) -> io::Result<()> {
		let result = self.inner.flush();
		(self.callback)(Event::Flush { result: &result });
		result
	}
}

impl<T: Seek, F: FnMut(Event<'_>)> Seek for Monitor<T, F> {
	fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
		let result = self.inner.seek(from);
		(self.callback)(Event::Seek {
			from,
			result: &result,
		});
		result
	}
}

/// A callback that prints events to [`std::io::Stderr`].
pub fn log_stderr(event: Event<'_>) {
	match event {
		Event::Read { data, result } => eprintln!("read {:?} => {:?}", data.len(), result),
		Event::Write { data, result } => eprintln!("write {:?} => {:?}", data.len(), result),
		Event::Flush { result } => eprintln!("flush => {:?}", result),
		Event::Seek { from, result } => eprintln!("seek {:?} => {:?}", from, result),
	}
}
