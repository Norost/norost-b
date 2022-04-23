use std::io::{self, Read, Seek, SeekFrom, Write};
use std::num::NonZeroU64;

/// A wrapper that prevents redundant seek calls from being performed. It also defers seeks until
/// a read or write is performed.
///
/// This assumes that the seek position advances at the same rate as data is being read & written.
pub struct CacheSeek<T: Seek> {
	/// The internal device to read & write to.
	inner: T,
	/// The position of the internal device. Mainly used to ensure consistency.
	inner_position: u64,
	/// The size of the underlying device. Evaluated lazily.
	inner_size: Option<NonZeroU64>,
	/// The current cached position.
	position: u64,
}

impl<T: Seek> CacheSeek<T> {
	/// Wrap a block device in a [`CacheSeek`].
	///
	/// The seek position of the inner device **must** be set to 0!
	pub fn new(inner: T) -> Self {
		Self {
			inner,
			inner_position: 0,
			inner_size: None,
			position: 0,
		}
	}

	/// Seek to the current position. This only performs an actual seek call if the given position
	/// doesn't match the current position.
	fn seek_exact(&mut self) -> io::Result<u64> {
		if self.inner_position != self.position {
			self.inner.seek(SeekFrom::Start(self.position))?;
			self.inner_position = self.position;
		}
		Ok(self.inner_position)
	}
}

impl<T: Seek + Read> Read for CacheSeek<T> {
	fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
		self.seek_exact()?;
		let l = self.inner.read(data)?;
		self.inner_position += u64::try_from(l).unwrap();
		self.position = self.inner_position;
		Ok(l)
	}
}

impl<T: Seek + Write> Write for CacheSeek<T> {
	fn write(&mut self, data: &[u8]) -> io::Result<usize> {
		self.seek_exact()?;
		let l = self.inner.write(data)?;
		self.inner_position += u64::try_from(l).unwrap();
		self.position = self.inner_position;
		Ok(l)
	}

	fn flush(&mut self) -> io::Result<()> {
		self.inner.flush()
	}
}

impl<T: Seek> Seek for CacheSeek<T> {
	fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
		self.position = match from {
			SeekFrom::Start(n) => n,
			SeekFrom::Current(d) => self.position.wrapping_add(d as u64),
			SeekFrom::End(n) => {
				let size = match self.inner_size {
					Some(n) => n.get(),
					None => {
						let n = self.inner.seek(SeekFrom::End(0))? + 1;
						self.inner_size = Some(n.try_into().unwrap());
						n
					}
				};
				size.wrapping_add(n as u64)
			}
		};
		Ok(self.position)
	}
}
