use core::num::Saturating;
use std::io::{self, Read, Write};
use term::{color::Color, Attr, Error, Terminal};

/// Simple ANSI terminal
pub struct AnsiTerminal<In, Out>
where
	In: Read,
	Out: Write,
{
	reader: In,
	writer: Out,
	prefix: Box<str>,
}

impl<In, Out> AnsiTerminal<In, Out>
where
	In: Read,
	Out: Write,
{
	pub fn new(reader: In, writer: Out) -> Self {
		Self {
			reader,
			writer,
			prefix: "".into(),
		}
	}

	pub fn set_prefix(&mut self, prefix: impl Into<Box<str>>) {
		self.prefix = prefix.into();
	}
}

impl<In, Out> Read for AnsiTerminal<In, Out>
where
	In: Read,
	Out: Write,
{
	/// Somewhat intelligent read function that accounts for backspace.
	fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
		match self.delete_line() {
			//Ok(_) => unsafe { core::arch::asm!("ud2") },
			Ok(_) => (),
			Err(Error::Io(e)) => return Err(e),
			Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
		};
		let prefix = std::mem::take(&mut self.prefix);
		self.write(prefix.as_bytes())?;
		self.prefix = prefix;

		let mut len = 0;
		loop {
			// Read
			let prev_len = len;
			len += self.reader.read(&mut data[len..])?;
			if len == prev_len {
				break Ok(0);
			}

			// Parse backspace & any annoying special characters
			for i in (prev_len..len).rev() {
				match data[i] {
					// 0x7f = delete, 0x8 = backspace
					0x7f | 0x8 => {
						let mut offt = Saturating(i);
						// include backspace char
						offt -= 1;
						// include UTF-8 char
						while offt.0 > 0 && data.get(offt.0).map_or(false, |c| c >> 6 == 0b10) {
							offt -= 1;
						}
						// remove
						data.copy_within(i + 1.., offt.0);
						len = len.saturating_sub(i - offt.0 + 1);
					}
					// Please don't break my terminal tyvm
					0x1b => {
						data.copy_within(i..data.len() - 1, i + 1);
						data[i] = b'^';
						data[i + 1] = b'[';
						len += 1;
					}
					_ => (),
				}
			}

			// Echo
			match self.delete_line() {
				Ok(_) => (),
				Err(Error::Io(e)) => return Err(e),
				Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
			};
			let prefix = std::mem::take(&mut self.prefix);
			self.write(prefix.as_bytes())?;
			self.prefix = prefix;
			self.write(&data[..len])?;

			// Check if line finished or buffer full
			if prev_len < len && data[prev_len..len].contains(&b'\n') || len >= data.len() {
				return Ok(len);
			}
		}
	}
}

impl<In, Out> Write for AnsiTerminal<In, Out>
where
	In: Read,
	Out: Write,
{
	fn write(&mut self, data: &[u8]) -> io::Result<usize> {
		self.writer.write(data)
	}

	fn flush(&mut self) -> io::Result<()> {
		self.writer.flush()
	}
}

impl<In, Out> term::Terminal for AnsiTerminal<In, Out>
where
	In: Read,
	Out: Write,
{
	type Output = Out;

	fn fg(&mut self, _color: Color) -> term::Result<()> {
		Err(Error::NotSupported)
	}

	fn bg(&mut self, _color: Color) -> term::Result<()> {
		Err(Error::NotSupported)
	}

	fn attr(&mut self, _attr: Attr) -> term::Result<()> {
		Err(Error::NotSupported)
	}

	fn supports_attr(&self, _attr: Attr) -> bool {
		false
	}

	fn reset(&mut self) -> term::Result<()> {
		Err(Error::NotSupported)
	}

	fn supports_reset(&self) -> bool {
		false
	}

	fn supports_color(&self) -> bool {
		false
	}

	fn cursor_up(&mut self) -> term::Result<()> {
		self.writer
			.write(b"\r\x1b[A")
			.map(|_| ())
			.map_err(Error::Io)
	}

	fn delete_line(&mut self) -> term::Result<()> {
		self.writer
			.write(b"\r\x1b[2K")
			.map(|_| ())
			.map_err(Error::Io)
	}

	fn carriage_return(&mut self) -> term::Result<()> {
		self.writer.write(b"\r").map(|_| ()).map_err(Error::Io)
	}

	fn get_ref(&self) -> &Self::Output {
		&self.writer
	}

	fn get_mut(&mut self) -> &mut Self::Output {
		&mut self.writer
	}

	fn into_inner(self) -> Self::Output {
		self.writer
	}
}
