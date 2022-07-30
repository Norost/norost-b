//! # I/O space access
//!
//! While using the TSS' IOPB is possible on x86 and x64, it is highly arch-specific and riddled
//! with legacy cruft that will inevitable lead to security issues[1].
//! Instead, a custom object is used which is simpler and very likely smaller in total space used.
//!
//! [1]: http://www.os2museum.com/wp/the-history-of-a-security-hole/

use crate::{
	arch::asm::io,
	object_table::{Error, Object, Root, SeekFrom, Ticket},
};
use alloc::{boxed::Box, sync::Arc};
use core::sync::atomic::{AtomicU16, Ordering};

pub fn post_init(root: &Root) {
	let io = Arc::new(Io) as Arc<dyn Object>;
	root.add(*b"portio", Arc::downgrade(&io));
	let _ = Arc::into_raw(io);
}

struct Io;

impl Object for Io {
	fn open(self: Arc<Self>, path: &[u8]) -> Ticket<Arc<dyn Object>> {
		Ticket::new_complete(if path == b"map" {
			Ok(Arc::new(IoMap { pos: 0.into() }))
		} else {
			Err(Error::DoesNotExist)
		})
	}
}

struct IoMap {
	pos: AtomicU16,
}

impl Object for IoMap {
	fn seek(&self, from: SeekFrom) -> Ticket<u64> {
		let mut pos = None;
		self.pos
			.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |p| {
				pos = Some(p);
				Some(from.apply(p.into(), u16::MAX.into()).try_into().unwrap())
			})
			.unwrap();
		Ticket::new_complete(Ok(pos.unwrap().into()))
	}

	fn read(self: Arc<Self>, length: usize) -> Ticket<Box<[u8]>> {
		let p = self.pos.load(Ordering::Relaxed);
		// SAFETY: nada *shrugs*
		unsafe {
			Ticket::new_complete(match length {
				1 => Ok(io::in8(p).to_le_bytes().into()),
				2 => Ok(io::in16(p).to_le_bytes().into()),
				4 => Ok(io::in32(p).to_le_bytes().into()),
				_ => Err(Error::InvalidData),
			})
		}
	}

	fn write(self: Arc<Self>, data: &[u8]) -> Ticket<u64> {
		let p = self.pos.load(Ordering::Relaxed);
		// SAFETY: *shrugs again*
		unsafe {
			match data {
				&[a] => io::out8(p, a),
				&[a, b] => io::out16(p, u16::from_le_bytes([a, b])),
				&[a, b, c, d] => io::out32(p, u32::from_le_bytes([a, b, c, d])),
				_ => return Ticket::new_complete(Err(Error::InvalidData)),
			}
		}
		Ticket::new_complete(Ok(data.len().try_into().unwrap()))
	}
}
