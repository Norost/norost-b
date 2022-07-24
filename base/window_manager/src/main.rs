//! # Tiling window manager
//!
//! This window manager is based on binary trees: each leaf is a window and each node is
//! grouped per two by a parent up to the root.
//!
//! ## Node paths
//!
//! A path has any of the the following syntaxes:
//!
//! ```
//! <workspace id/name>:<window id>
//! ```

#![cfg_attr(not(test), no_std)]
#![feature(alloc_error_handler)]
#![feature(core_intrinsics)]
#![feature(start)]

extern crate alloc;

mod manager;
mod math;
mod window;
mod workspace;

use alloc::vec::Vec;
use core::{
	cell::RefCell,
	ptr::{self, NonNull},
	str,
};
use driver_utils::os::stream_table::{JobId, Request, Response, StreamTable};
use math::{Point, Rect, Size};
use rt::io::{Error, Handle};

#[cfg(not(test))]
#[global_allocator]
static ALLOC: rt_alloc::Allocator = rt_alloc::Allocator;

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error(_: core::alloc::Layout) -> ! {
	// FIXME the runtime allocates memory by default to write things, so... crap
	// We can run in similar trouble with the I/O queue. Some way to submit I/O requests
	// without going through an queue may be useful.
	rt::exit(129)
}

#[cfg(not(test))]
#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
	let _ = rt::io::stderr().map(|o| writeln!(o, "{}", info));
	rt::exit(128)
}

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let root = rt::io::file_root().unwrap();
	let sync = root.open(b"gpu/sync").unwrap();
	let res = {
		let mut b = [0; 8];
		let l = sync
			.get_meta(b"bin/resolution".into(), (&mut b).into())
			.unwrap();
		ipc_gpu::Resolution::decode(b)
	};
	let size = Size::new(res.x, res.y);

	let shmem_size = size.x as usize * size.y as usize * 3;
	let shmem_size = (shmem_size + 0xfff) & !0xfff;
	let shmem_obj = rt::Object::new(rt::io::NewObject::SharedMemory { size: shmem_size }).unwrap();
	let (shmem, shmem_size) = shmem_obj
		.map_object(None, rt::io::RWX::RW, 0, shmem_size)
		.unwrap();
	sync.share(
		&rt::Object::new(rt::io::NewObject::PermissionMask {
			handle: shmem_obj.as_raw(),
			rwx: rt::io::RWX::R,
		})
		.unwrap(),
	)
	.expect("failed to share mem with GPU");
	// SAFETY: only we can write to this slice. The other side can go figure.
	let shmem = unsafe { core::slice::from_raw_parts_mut(shmem.as_ptr(), shmem_size) };

	let gwp = window::GlobalWindowParams { border_width: 4 };
	let mut manager = manager::Manager::<Client>::new(gwp).unwrap();

	let sync_rect = |rect: math::Rect| {
		sync.write(
			&ipc_gpu::Flush {
				offset: 0,
				stride: rect.size().x,
				origin: ipc_gpu::Point {
					x: rect.low().x,
					y: rect.low().y,
				},
				size: ipc_gpu::SizeInclusive {
					x: rect.size().x as _,
					y: rect.size().y as _,
				},
			}
			.encode(),
		)
		.unwrap();
	};
	let mut fill = |shmem: &mut [u8], rect: math::Rect, color: [u8; 3]| {
		let t = rect.size();
		assert!(t.x <= size.x && t.y <= size.y, "rect out of bounds");
		assert!(t.area() * 3 <= shmem.len() as u64, "shmem too small");
		for y in 0..t.y {
			for x in 0..t.x {
				let i = y * t.x + x;
				let s = &mut shmem[i as usize * 3..][..3];
				s.copy_from_slice(&color);
			}
		}
		sync_rect(rect);
	};

	let colors = [
		[255, 0, 0],
		[0, 255, 0],
		[0, 0, 255],
		[255, 255, 0],
		[0, 255, 255],
		[255, 0, 255],
	];

	let tbl_buf = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 12 }).unwrap();
	let mut table = StreamTable::new(&tbl_buf, rt::io::Pow2Size(5));
	root.create(b"window_manager")
		.unwrap()
		.share(table.public())
		.unwrap();

	let mut prop_buf = [0; 511];
	loop {
		let mut send_notif = false;
		while let Some((handle, req)) = table.dequeue() {
			let (job_id, response) = match req {
				Request::Create { job_id, path } => (job_id, {
					let mut p = [0; 8];
					let p = &mut p[..path.len()];
					path.copy_to(0, p);
					path.manual_drop();
					match (handle, &*p) {
						(Handle::MAX, b"window") => {
							let h = manager.new_window(size, Default::default()).unwrap();
							fill(shmem, Rect::from_size(Point::ORIGIN, size), [50, 50, 50]);
							for ((w, ww), c) in manager.windows().zip(&colors) {
								let rect = manager.window_rect(w, size).unwrap();
								let evt = ipc_wm::Resolution {
									x: rect.size().x,
									y: rect.size().y,
								};
								let mut ue = ww.user_data.unread_events.borrow_mut();
								ue.resize = Some(evt);
								let evt = ipc_wm::Event::Resize(evt).encode();
								for id in ww.user_data.event_listeners.borrow_mut().drain(..) {
									ue.resize = None;
									let data = table.alloc(evt.len()).expect("out of buffers");
									data.copy_from(0, &evt);
									table.enqueue(id, Response::Data(data));
									send_notif = true;
								}
							}
							Response::Handle(h)
						}
						_ => Response::Error(Error::InvalidOperation),
					}
				}),
				Request::GetMeta { job_id, property } => {
					let prop = property.get(&mut prop_buf);
					property.manual_drop();
					let r = match (handle, &*prop) {
						(Handle::MAX, _) => Response::Error(Error::InvalidOperation as _),
						(h, b"bin/resolution") => {
							let rect = manager.window_rect(h, size).unwrap();
							let data = table.alloc(8).expect("out of buffers");
							data.copy_from(0, &u32::from(rect.size().x).to_le_bytes());
							data.copy_from(4, &u32::from(rect.size().y).to_le_bytes());
							Response::Data(data)
						}
						(_, _) => Response::Error(Error::DoesNotExist as _),
					};
					(job_id, r)
				}
				Request::SetMeta {
					job_id,
					property_value,
				} => {
					let (prop, val) = property_value.try_get(&mut prop_buf).unwrap();
					property_value.into_inner().manual_drop();
					let r = match (handle, &*prop) {
						(Handle::MAX, _) => Response::Error(Error::InvalidOperation as _),
						(h, b"bin/cmd/fill") => {
							if let &[r, g, b] = &*val {
								fill(shmem, manager.window_rect(h, size).unwrap(), [r, g, b]);
								Response::Amount(0)
							} else {
								Response::Error(Error::InvalidData)
							}
						}
						(_, _) => Response::Error(Error::DoesNotExist as _),
					};
					(job_id, r)
				}
				Request::Read {
					job_id,
					amount: _,
					peek: _,
				} => (
					job_id,
					match handle {
						Handle::MAX => Response::Error(Error::InvalidOperation),
						h => {
							let w = &mut manager.window_mut(handle).unwrap().user_data;
							if let Some(evt) = w.unread_events.get_mut().pop() {
								let evt = evt.encode();
								let data = table.alloc(evt.len()).expect("out of buffers");
								data.copy_from(0, &evt);
								Response::Data(data)
							} else {
								w.event_listeners.get_mut().push(job_id);
								continue;
							}
						}
					},
				),
				Request::Write { job_id, data } => (
					job_id,
					match handle {
						Handle::MAX => Response::Error(Error::InvalidOperation),
						h => {
							let window = manager.window(handle).unwrap();
							let mut header = [0; 12];
							data.copy_to(0, &mut header);
							let display = Rect::from_size(Point::ORIGIN, size);
							let rect = manager.window_rect(h, size).unwrap();
							let draw = ipc_wm::Flush::decode(header);
							let draw_size = draw.size;
							// TODO do we actually want this?
							let draw_size = Size::new(
								(u32::from(draw_size.x) + 1).min(rect.size().x),
								(u32::from(draw_size.y) + 1).min(rect.size().y),
							);
							let draw_orig = draw.origin;
							let draw_orig = Point::new(draw_orig.x, draw_orig.y);
							let draw_rect = rect
								.calc_global_pos(Rect::from_size(draw_orig, draw_size))
								.unwrap();
							debug_assert_eq!((0..draw_size.x).count(), draw_rect.x().count());
							debug_assert_eq!((0..draw_size.y).count(), draw_rect.y().count());
							assert!(
								draw_rect.high().x * size.y as u32 + draw_rect.high().y
									<= size.x * size.y
							);
							// TODO we can avoid this copy by passing shared memory buffers directly
							// to the GPU
							window
								.user_data
								.framebuffer
								.copy_to_untrusted(0, &mut shmem[..draw_rect.area() as usize * 3]);
							let l = data.len().try_into().unwrap();
							data.manual_drop();
							sync_rect(draw_rect);
							Response::Amount(l)
						}
					},
				),
				Request::Close => {
					manager.destroy_window(handle).unwrap();
					fill(shmem, Rect::from_size(Point::ORIGIN, size), [50, 50, 50]);
					for ((w, ww), c) in manager.windows().zip(&colors) {
						let rect = manager.window_rect(w, size).unwrap();
						let evt = ipc_wm::Resolution {
							x: rect.size().x,
							y: rect.size().y,
						};
						let mut ue = ww.user_data.unread_events.borrow_mut();
						ue.resize = Some(evt);
						let evt = ipc_wm::Event::Resize(evt).encode();
						for id in ww.user_data.event_listeners.borrow_mut().drain(..) {
							ue.resize = None;
							let data = table.alloc(evt.len()).expect("out of buffers");
							data.copy_from(0, &evt);
							table.enqueue(id, Response::Data(data));
							send_notif = true;
						}
					}
					continue;
				}
				Request::Open { .. } => todo!(),
				Request::Share { job_id, share } => {
					let r = match handle {
						Handle::MAX => Response::Error(Error::InvalidOperation),
						h => {
							manager.window_mut(handle).unwrap().user_data.framebuffer =
								FrameBuffer::wrap(&share);
							Response::Amount(0)
						}
					};
					(job_id, r)
				}
				_ => todo!(),
			};
			table.enqueue(job_id, response);
			send_notif = true;
		}
		send_notif.then(|| table.flush());
		table.wait();
	}
}

struct FrameBuffer {
	base: NonNull<u8>,
	size: usize,
}

impl FrameBuffer {
	fn wrap(obj: &rt::Object) -> Self {
		let (base, size) = obj.map_object(None, rt::io::RWX::R, 0, usize::MAX).unwrap();
		Self { base, size }
	}

	fn copy_to_untrusted(&self, offset: usize, out: &mut [u8]) {
		assert!(
			offset < self.size && offset + out.len() <= self.size,
			"out of bounds"
		);
		unsafe {
			core::intrinsics::volatile_copy_nonoverlapping_memory(
				out.as_mut_ptr(),
				self.base.as_ptr().add(offset),
				out.len(),
			);
		}
	}
}

impl Drop for FrameBuffer {
	fn drop(&mut self) {
		if self.size > 0 {
			// SAFETY: we have unique ownership of the memory.
			unsafe {
				// Assume nothing bad will happen
				let _ = rt::mem::dealloc(self.base, self.size);
			}
		}
	}
}

impl Default for FrameBuffer {
	fn default() -> Self {
		Self {
			base: NonNull::dangling(),
			size: 0,
		}
	}
}

#[derive(Default)]
struct Client {
	framebuffer: FrameBuffer,
	unread_events: RefCell<Events>,
	event_listeners: RefCell<Vec<JobId>>,
}

#[derive(Default)]
struct Events {
	resize: Option<ipc_wm::Resolution>,
}

impl Events {
	fn push(&mut self, e: ipc_wm::Event) {
		match e {
			ipc_wm::Event::Resize(r) => self.resize = Some(r),
		}
	}

	fn pop(&mut self) -> Option<ipc_wm::Event> {
		self.resize.take().map(ipc_wm::Event::Resize)
	}
}
