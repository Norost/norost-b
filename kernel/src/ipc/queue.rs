use crate::memory::frame;
use crate::memory::r#virtual::Mappable;
use crate::memory::Page;
use core::alloc::Layout;
use core::cell::Cell;
use core::mem;
use core::ptr::NonNull;
use core::num::NonZeroUsize;
use core::slice;
use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy)]
#[repr(C)]
#[repr(align(64))] // Align to 64 bytes to avoid false sharing as much as possible
pub struct SubmissionEntry {
	pub opcode: u8,
	pub data: [u8; 64 - mem::size_of::<u8>() - mem::size_of::<u64>()],
	pub user_data: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
#[repr(align(16))] // Align to 16 bytes to avoid false sharing as much as possible
pub struct CompletionEntry {
	user_data: u64,
	_data: u64,
}

pub struct ClientQueue {
	submission_tail: u32,
	submission_mask: u32,
	completion_head: u32,
	completion_mask: u32,
	queues: NonNull<u8>,
}

#[repr(C)]
pub struct ClientQueueHeader {
	submission_head: AtomicU32,
	completion_head: AtomicU32,
	// The fields below are for the process' convienence and not used by the kernel
	submission_mask: u32,
	completion_mask: u32,
	submission_offset: u32,
	completion_offset: u32,
}

impl ClientQueue {
	pub fn new(sq_p2size: usize, cq_p2size: usize) -> Result<Self, NewClientQueueError> {
		if sq_p2size > 15 || cq_p2size > 15 {
			Err(NewClientQueueError::SizeTooLarge)?;
		}

		let sq_size = 1u32 << sq_p2size;
		let cq_size = 1u32 << cq_p2size;
		let ring_layout = Self::ring_layout(sq_size, cq_size);

		let pages = Page::min_pages_for_bytes(ring_layout.size());
		let queues = frame::allocate_contiguous(NonZeroUsize::new(pages).unwrap())
			.map_err(NewClientQueueError::AllocateError)?
			.as_ptr();

		Ok(Self {
			submission_tail: 0,
			submission_mask: sq_size - 1,
			completion_head: 0,
			completion_mask: cq_size - 1,
			queues: NonNull::new(queues).unwrap().cast(),
		})
	}

	/// Pop a submitted entry.
	///
	/// The entire value is copied since the user process might be naughty and modify the entry
	/// when it's already submitted. Reading and writing unatomically from two threads
	/// simultaneously is undefined behaviour and we'd very much like to avoid that.
	pub fn pop_submission(&mut self) -> Option<SubmissionEntry> {
		let head = self.header().submission_head.load(Ordering::Acquire);
		let tail = self.submission_tail & self.completion_mask;
		(head != tail).then(|| {
			let entry = &self.submissions()[usize::try_from(tail).unwrap()];
			let value = entry.get();
			// compiler_fence emits code: https://github.com/rust-lang/rust/issues/62256
			// the asm! macro can perform a compiler memory barrier without emitting code
			unsafe {
				asm!("# {0}", in(reg) entry);
			}
			self.submission_tail = self.submission_tail.wrapping_add(1);
			value
		})
	}

	/// Push a completion entry.
	pub fn push_completion(&mut self, entry: CompletionEntry) {
		let head = usize::try_from(self.completion_head & self.completion_mask).unwrap();
		self.completions()[head].set(entry);
		self.completion_head = self.completion_head.wrapping_add(1);
		self.header()
			.completion_head
			.store(self.completion_head, Ordering::Release);
	}

	/// Return the submissions ring.
	///
	/// Since we share the submissions with a process which may be naughty we have to use `Cell`.
	fn submissions(&self) -> &[Cell<SubmissionEntry>] {
		unsafe {
			let slen = usize::try_from(self.completion_mask).unwrap() + 1;
			let ptr = self
				.queues
				.as_ptr()
				.cast::<ClientQueueHeader>()
				.add(1)
				.cast();
			slice::from_raw_parts(ptr, slen)
		}
	}

	/// Return the completions ring.
	///
	/// Since we share the completions with a process which may be naughty we have to use `Cell`.
	fn completions(&self) -> &[Cell<CompletionEntry>] {
		unsafe {
			let slen = usize::try_from(self.completion_mask).unwrap() + 1;
			let clen = usize::try_from(self.completion_mask).unwrap() + 1;
			let ptr = self
				.queues
				.as_ptr()
				.cast::<ClientQueueHeader>()
				.add(1)
				.cast::<Cell<SubmissionEntry>>()
				.add(slen)
				.cast();
			slice::from_raw_parts(ptr, clen)
		}
	}

	fn header(&self) -> &ClientQueueHeader {
		unsafe { self.queues.cast().as_ref() }
	}

	fn ring_layout(sq_size: u32, cq_size: u32) -> Layout {
		let header_layout = Layout::new::<ClientQueueHeader>();
		let sq_layout = Layout::array::<SubmissionEntry>(sq_size.try_into().unwrap()).unwrap();
		let cq_layout = Layout::array::<CompletionEntry>(cq_size.try_into().unwrap()).unwrap();
		header_layout
			.extend(sq_layout)
			.unwrap()
			.0
			.extend(cq_layout)
			.unwrap()
			.0
	}
}

impl Drop for ClientQueue {
	fn drop(&mut self) {
		todo!()
	}
}

unsafe impl Mappable<Iter> for ClientQueue {
	fn len(&self) -> usize {
		let layout = Self::ring_layout(self.submission_mask + 1, self.completion_mask + 1);
		Page::min_pages_for_bytes(layout.size())
	}

	fn frames(&self) -> Iter {
		Iter(
			unsafe { frame::PPN::from_ptr(self.queues.as_ptr().cast()) },
			self.len(),
		)
	}
}

pub struct Iter(frame::PPN, usize);

impl Iterator for Iter {
	type Item = frame::PPN;

	fn next(&mut self) -> Option<Self::Item> {
		(self.1 > 0).then(|| {
			self.1 -= 1;
			let ppn = self.0;
			self.0 = self.0.next();
			ppn
		})
	}
}

impl ExactSizeIterator for Iter {
	fn len(&self) -> usize {
		self.1
	}
}

#[derive(Debug)]
pub enum NewClientQueueError {
	SizeTooLarge,
	AllocateError(frame::AllocateContiguousError),
}
