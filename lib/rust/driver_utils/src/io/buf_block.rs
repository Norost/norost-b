use super::CacheSeek;
use std::io::{self, BufRead, Read, ReadBuf, Seek, SeekFrom, Write};
use std::mem::MaybeUninit;

/// A buffered I/O wrapper for block devices.
///
/// It reads & buffer an entire power-of-two aligned block of data.
///
/// Unlike other I/O buffer wrappers such as [`fscommon::BufStream`] this type does
/// not invalidate its buffer when a seek is performed.
pub struct BufBlock<T: Read + Write + Seek> {
	/// The internal device to read & write to.
	inner: CacheSeek<T>,
	/// Whether we did a write to the internal data.
	inner_dirty: bool,
	/// Buffered data.
	buffer: [MaybeUninit<u8>; 512],
	/// The corresponding position of the buffered data relative to the device's data.
	buffer_position: u64,
	/// How many bytes of data are valid in the buffer.
	buffer_valid: usize,
	/// Whether any data has been written to the buffer.
	buffer_dirty: bool,
	/// The position as seen by the user of this [`BufBlock`].
	position: u64,
}

impl<T: Read + Write + Seek> BufBlock<T> {
	/// Wrap a block device in a [`BufBlock`].
	///
	/// The seek position of the inner device **must** be set to 0!
	pub fn new(inner: T) -> Self {
		Self {
			inner: CacheSeek::new(inner),
			inner_dirty: false,
			buffer: MaybeUninit::uninit_array(),
			// The buffer is a power of two size and the corresponding position
			// is always aligned on a power of two border, so u64::MAX is an invalid sector
			// as the lower bits are all ones.
			buffer_position: u64::MAX,
			buffer_valid: 0,
			buffer_dirty: false,
			position: 0,
		}
	}

	fn buffer_mask(&self) -> usize {
		self.buffer.len() - 1
	}

	fn calc_buffer_offset(&self, pos: u64) -> usize {
		(pos & u64::try_from(self.buffer_mask()).unwrap())
			.try_into()
			.unwrap()
	}

	fn calc_buffer_position(&self, pos: u64) -> u64 {
		pos & !u64::try_from(self.buffer_mask()).unwrap()
	}

	/// Return the current valid range of the buffer.
	fn buffer(&self) -> &[u8] {
		unsafe {
			let range = self.calc_buffer_offset(self.position)..self.buffer_valid;
			MaybeUninit::slice_assume_init_ref(&self.buffer[range])
		}
	}

	/// The available capacity given the current position.
	fn capacity(&self) -> usize {
		self.buffer.len() - self.calc_buffer_offset(self.position)
	}

	/// Write out the internal buffer. This does not send a flush command to the internal
	/// device.
	fn flush_buffer(&mut self) -> io::Result<()> {
		if self.buffer_dirty {
			self.inner.seek(SeekFrom::Start(self.buffer_position))?;
			let mut written = 0;
			while written < self.buffer_valid {
				let buf = unsafe {
					MaybeUninit::slice_assume_init_ref(&self.buffer[written..self.buffer_valid])
				};
				let l = self.inner.write(buf)?;
				written += l;
				self.inner_dirty = true;
			}
			self.buffer_dirty = false;
		}
		Ok(())
	}
}

impl<T: Read + Write + Seek> BufRead for BufBlock<T> {
	fn fill_buf(&mut self) -> io::Result<&[u8]> {
		let new_position = self.calc_buffer_position(self.position);
		if self.buffer_position != new_position {
			self.flush_buffer()?;
			let mut buf = ReadBuf::uninit(&mut self.buffer);
			self.inner.seek(SeekFrom::Start(new_position))?;
			self.inner.read_buf(&mut buf)?;
			self.buffer_valid = buf.filled_len();
			self.buffer_position = new_position;
		}
		Ok(self.buffer())
	}

	fn consume(&mut self, n: usize) {
		let n = u64::try_from(n).unwrap();
		let valid = u64::try_from(self.buffer_valid).unwrap();
		self.position = (self.position + n).min(self.calc_buffer_position(self.position) + valid);
	}
}

impl<T: Read + Write + Seek> Read for BufBlock<T> {
	fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
		if data.len() > self.capacity() {
			// We can't buffer all the data, so perform a direct read.
			let len = self.inner.read(data)?;
			self.position += u64::try_from(len).unwrap();
			Ok(len)
		} else {
			// We have buffered all the data.
			let buf = self.fill_buf()?;
			let len = buf.len().min(data.len());
			data[..len].copy_from_slice(&buf[..len]);
			self.consume(len);
			Ok(len)
		}
	}
}

impl<T: Read + Write + Seek> Write for BufBlock<T> {
	fn write(&mut self, data: &[u8]) -> io::Result<usize> {
		if data.len() > self.capacity() {
			// We can't buffer all the data, so perform a direct write.
			let len = self.inner.write(data)?;
			self.position += u64::try_from(len).unwrap();
			self.inner_dirty = true;
			Ok(len)
		} else {
			// Ensure we're writing into the right sector, which may require a read first.
			self.fill_buf()?;
			let offset = self.calc_buffer_offset(self.position);
			let mut buf = ReadBuf::uninit(&mut self.buffer[offset..]);
			buf.append(data);
			self.consume(data.len());
			self.buffer_dirty = true;
			Ok(data.len())
		}
	}

	fn flush(&mut self) -> io::Result<()> {
		self.flush_buffer()?;
		if self.inner_dirty {
			self.inner.flush()?;
			self.inner_dirty = false;
		}
		Ok(())
	}
}

impl<T: Read + Write + Seek> Seek for BufBlock<T> {
	fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
		self.position = self.inner.seek(from)?;
		Ok(self.position)
	}
}
