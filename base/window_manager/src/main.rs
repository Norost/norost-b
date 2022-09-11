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

#![feature(core_intrinsics)]
#![feature(norostb)]
#![feature(let_else)]

mod config;
mod manager;
mod math;
mod title_bar;
mod window;
mod workspace;

use {
	core::{cell::RefCell, ptr::NonNull, time::Duration},
	driver_utils::{
		os::stream_table::{JobId, Request, Response, StreamTable},
		task,
	},
	io_queue_rt::{Pow2Size, Queue},
	math::{Point, Rect, Size, Vector},
	rt::io::{Error, Handle},
	std::collections::VecDeque,
};

pub struct Main<'a> {
	size: Size,
	shmem: &'a mut [u8],
	sync: rt::RefObject<'a>,
}

impl Main<'_> {
	fn fill(&mut self, rect: Rect, color: [u8; 3]) {
		let t = rect.size();
		assert!(
			t.x <= self.size.x && t.y <= self.size.y,
			"rect out of bounds"
		);
		assert!(t.area() * 3 <= self.shmem.len() as u64, "shmem too small");
		for y in 0..t.y {
			for x in 0..t.x {
				let i = y * t.x + x;
				let s = &mut self.shmem[i as usize * 3..][..3];
				s.copy_from_slice(&color);
			}
		}
		self.sync_rect(rect);
	}

	fn sync_rect(&mut self, rect: Rect) {
		self.sync
			.write(
				&ipc_gpu::Flush {
					offset: 0,
					stride: rect.size().x,
					origin: ipc_gpu::Point { x: rect.low().x, y: rect.low().y },
					size: ipc_gpu::SizeInclusive { x: rect.size().x as _, y: rect.size().y as _ },
				}
				.encode(),
			)
			.unwrap();
	}

	fn copy(&mut self, data: &[u8], to: Rect) {
		self.shmem[..data.len()].copy_from_slice(data);
		self.sync_rect(to);
	}
}

fn main() {
	let config = config::load();

	let sync = rt::args::handle(b"gpu").expect("gpu undefined");
	let res = {
		let mut b = [0; 8];
		sync.get_meta(b"bin/resolution".into(), (&mut b).into())
			.unwrap();
		ipc_gpu::Resolution::decode(b)
	};
	let size = Size::new(res.x, res.y);

	let shmem_size = size.x as usize * size.y as usize * 3;
	let shmem_size = (shmem_size + 0xfff) & !0xfff;
	let (shmem_obj, _) =
		rt::Object::new(rt::io::NewObject::SharedMemory { size: shmem_size }).unwrap();
	let (shmem, shmem_size) = shmem_obj
		.map_object(None, rt::io::RWX::RW, 0, shmem_size)
		.unwrap();
	sync.share(
		&rt::Object::new(rt::io::NewObject::PermissionMask {
			handle: shmem_obj.as_raw(),
			rwx: rt::io::RWX::R,
		})
		.unwrap()
		.0,
	)
	.expect("failed to share mem with GPU");
	// SAFETY: only we can write to this slice. The other side can go figure.
	let shmem = unsafe { core::slice::from_raw_parts_mut(shmem.as_ptr(), shmem_size) };

	let gwp = window::GlobalWindowParams { margin: config.margin };
	let mut manager = manager::Manager::<Client>::new(gwp).unwrap();

	let mut main = Main { size, sync, shmem };

	let (tbl_buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 12 }).unwrap();
	let table = StreamTable::new(&tbl_buf, rt::io::Pow2Size(5), (1 << 8) - 1);
	rt::args::handle(b"share")
		.expect("share undefined")
		.share(table.public())
		.expect("failed to share");

	{
		let c = &config.cursor;
		let r = c.as_raw();
		main.shmem[..r.len()].copy_from_slice(r);
		let f = |n| u8::try_from(n - 1).unwrap();
		sync.write(&[0xc5, f(c.width()), f(c.height())]).unwrap();
	}

	let mut mouse_pos = Point::new((size.x / 2).into(), (size.y / 2).into());
	let [a, b] = (mouse_pos.x as u16).to_le_bytes();
	let [c, d] = (mouse_pos.y as u16).to_le_bytes();
	sync.set_meta(b"bin/cursor/pos".into(), (&[a, b, c, d]).into())
		.unwrap();

	let queue = Queue::new(Pow2Size::P2, Pow2Size::P2).unwrap();
	let mut poll_table = queue.submit_read(table.notifier().as_raw(), ()).unwrap();

	let mut old = None;

	let mut mouse_clicked = false;

	loop {
		queue.poll();
		queue.wait(Duration::MAX);
		queue.process();

		if task::poll(&mut poll_table).is_some() {
			poll_table = queue.submit_read(table.notifier().as_raw(), ()).unwrap();
		}
		let mut send_notif = false;
		let mut draw_focus_borders = None;

		const INPUT: Handle = Handle::MAX - 1;

		while let Some((handle, job_id, req)) = table.dequeue() {
			let mut prop_buf = [0; 511];
			let response = match req {
				Request::Create { path } => {
					let mut p = [0; 8];
					let (p, _) = path.copy_into(&mut p);
					match (handle, &*p) {
						(Handle::MAX, b"window") => {
							let h = manager.new_window(size, Default::default()).unwrap();
							main.fill(Rect::from_size(Point::ORIGIN, size), [50, 50, 50]);
							old = None;
							for (w, ww) in manager.windows() {
								let full_rect = manager.window_rect(w, size).unwrap();
								let (title, rect) = title_bar::split(&config, full_rect);
								title_bar::render(
									&mut main,
									&config,
									title,
									mouse_pos,
									&ww.user_data.title,
								);
								let evt = ipc_wm::Resolution { x: rect.size().x, y: rect.size().y };
								let mut ue = ww.user_data.unread_events.borrow_mut();
								ue.resize = Some(evt);
								let evt = ipc_wm::Event::Resize(evt).encode();
								for id in ww.user_data.event_listeners.borrow_mut().drain(..) {
									ue.resize = None;
									let data = table.alloc(evt.len()).expect("out of buffers");
									data.copy_from(0, &evt);
									table.enqueue(id, Response::Data(data));
								}
								if Some(w) == manager.focused_window() {
									draw_focus_borders = Some(full_rect);
								}
							}
							Response::Handle(h)
						}
						_ => Response::Error(Error::InvalidOperation),
					}
				}
				Request::GetMeta { property } => {
					let prop = property.get(&mut prop_buf);
					match (handle, &*prop) {
						(Handle::MAX, _) => Response::Error(Error::InvalidOperation as _),
						(h, b"bin/resolution") => {
							let rect = manager.window_rect(h, size).unwrap();
							let (_, rect) = title_bar::split(&config, rect);
							let data = table.alloc(8).expect("out of buffers");
							data.copy_from(0, &u32::from(rect.size().x).to_le_bytes());
							data.copy_from(4, &u32::from(rect.size().y).to_le_bytes());
							Response::Data(data)
						}
						(_, _) => Response::Error(Error::DoesNotExist as _),
					}
				}
				Request::SetMeta { property_value } => {
					let (prop, val) = property_value.try_get(&mut prop_buf).unwrap();
					match (handle, &*prop) {
						(Handle::MAX, _) => Response::Error(Error::InvalidOperation as _),
						(h, b"bin/cmd/fill") => {
							if let &[r, g, b] = &*val {
								let rect = manager.window_rect(h, size).unwrap();
								let (_, rect) = title_bar::split(&config, rect);
								main.fill(rect, [r, g, b]);
								Response::Amount(0)
							} else {
								Response::Error(Error::InvalidData)
							}
						}
						(h, b"title") => {
							let s = String::from_utf8_lossy(val).into_owned().into_boxed_str();
							let r = manager.window_rect(h, size).unwrap();
							let (r, _) = title_bar::split(&config, r);
							title_bar::render(&mut main, &config, r, mouse_pos, &s);
							manager.window_mut(h).unwrap().user_data.title = s;
							Response::Amount(0)
						}
						(_, _) => Response::Error(Error::DoesNotExist as _),
					}
				}
				Request::Read { amount: _ } if handle != Handle::MAX => {
					let w = &mut manager.window_mut(handle).unwrap().user_data;
					if let Some(evt) = w.unread_events.get_mut().pop() {
						let evt = evt.encode();
						let data = table.alloc(evt.len()).expect("out of buffers");
						data.copy_from(0, &evt);
						Response::Data(data)
					} else {
						w.event_listeners.get_mut().push_back(job_id);
						continue;
					}
				}
				Request::Write { data } if handle == INPUT => {
					use input::{Input, Movement, Type};
					let mut mouse_moved = false;
					let mouse_was_clicked = mouse_clicked;
					for (_, b) in data.blocks() {
						for i in (0..b.len() / 8).map(|i| i * 8) {
							let mut buf = [0; 8];
							b.copy_to(i, &mut buf);
							let k = u64::from_le_bytes(buf);
							let Ok(k) = Input::try_from(k) else { continue };
							let l = k.press_level;
							match k.ty {
								Type::Relative(0, Movement::TranslationX) => {
									mouse_pos.x = if l >= 0 {
										(mouse_pos.x + l as u32).min(size.x - 1)
									} else {
										mouse_pos.x.saturating_sub(-l as u32)
									};
									mouse_moved = true;
								}
								Type::Relative(0, Movement::TranslationY) => {
									mouse_pos.y = if l >= 0 {
										mouse_pos.y.saturating_sub(l as u32)
									} else {
										(mouse_pos.y + -l as u32).min(size.y - 1)
									};
									mouse_moved = true;
								}
								Type::Absolute(0, Movement::TranslationX) => {
									mouse_pos.x = (l as u64 * size.x as u64 / (1 << 31)) as _;
									mouse_moved = true;
								}
								Type::Absolute(0, Movement::TranslationY) => {
									mouse_pos.y = (l as u64 * size.y as u64 / (1 << 31)) as _;
									mouse_moved = true;
								}
								Type::Button(0) => mouse_clicked = k.is_press(),
								_ => {
									let Some(w) = manager.focused_window() else { continue };
									let u = &mut manager.window_mut(w).unwrap().user_data;
									if let Some(id) = u.event_listeners.get_mut().pop_front() {
										let evt = ipc_wm::Event::Input(k).encode();
										let d = table.alloc(evt.len()).expect("out of buffers");
										d.copy_from(0, &evt);
										table.enqueue(id, Response::Data(d));
									} else {
										u.unread_events.get_mut().inputs.push_back(k);
									}
								}
							};
						}
					}
					let edge = !mouse_was_clicked & mouse_clicked;
					if mouse_moved {
						let [a, b] = (mouse_pos.x as u16).to_le_bytes();
						let [c, d] = (mouse_pos.y as u16).to_le_bytes();
						sync.set_meta(b"bin/cursor/pos".into(), (&[a, b, c, d]).into())
							.unwrap();
						for (w, ww) in manager.windows() {
							let full_rect = manager.window_rect(w, size).unwrap();
							let (title, rect) = title_bar::split(&config, full_rect);
							let close = title_bar::Button::Close.render(
								&mut main,
								&config,
								title,
								mouse_pos,
								mouse_clicked,
							);
							title_bar::Button::Maximize.render(
								&mut main,
								&config,
								title,
								mouse_pos,
								mouse_clicked,
							);
							if edge & close {
								let u = &ww.user_data;
								if let Some(id) = u.event_listeners.borrow_mut().pop_front() {
									let evt = ipc_wm::Event::Close.encode();
									let d = table.alloc(evt.len()).expect("out of buffers");
									d.copy_from(0, &evt);
									table.enqueue(id, Response::Data(d));
								} else {
									u.unread_events.borrow_mut().close = true;
								}
							}
						}
					}
					if edge {
						let (h, r) = manager.window_at(mouse_pos, size).unwrap();
						if Some(h) != manager.focused_window() {
							manager.set_focused_window(h);
							let r = Rect::from_points(
								r.low() + Vector::ONE * config.margin,
								r.high() - Vector::ONE * config.margin,
							);
							draw_focus_borders = Some(r);
						}
					}
					Response::Amount(data.len() as _)
				}
				Request::Write { data } if handle != Handle::MAX => {
					let window = manager.window(handle).unwrap();
					let mut header = [0; 12];
					data.copy_to(0, &mut header);
					let rect = manager.window_rect(handle, size).unwrap();
					let (_, rect) = title_bar::split(&config, rect);
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
						draw_rect.high().x * size.y as u32 + draw_rect.high().y <= size.x * size.y
					);
					// TODO we can avoid this copy by passing shared memory buffers directly
					// to the GPU
					window
						.user_data
						.framebuffer
						.copy_to_untrusted(0, &mut main.shmem[..draw_rect.area() as usize * 3]);
					let l = data.len().try_into().unwrap();
					main.sync_rect(draw_rect);
					Response::Amount(l)
				}
				Request::Close if handle != INPUT => {
					manager.destroy_window(handle).unwrap();
					main.fill(Rect::from_size(Point::ORIGIN, size), [50, 50, 50]);
					old = None;
					for (w, ww) in manager.windows() {
						let full_rect = manager.window_rect(w, size).unwrap();
						let (title, rect) = title_bar::split(&config, full_rect);
						title_bar::render(
							&mut main,
							&config,
							title,
							mouse_pos,
							&ww.user_data.title,
						);
						let evt = ipc_wm::Resolution { x: rect.size().x, y: rect.size().y };
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
						if Some(w) == manager.focused_window() {
							draw_focus_borders = Some(full_rect);
						}
					}
					continue;
				}
				Request::Close => continue,
				Request::Open { path } if handle == Handle::MAX => {
					match &*path.copy_into(&mut [0; 16]).0 {
						b"input" => Response::Handle(INPUT),
						_ => Response::Error(Error::DoesNotExist),
					}
				}
				Request::Share { share } if handle != Handle::MAX => {
					manager.window_mut(handle).unwrap().user_data.framebuffer =
						FrameBuffer::wrap(&share);
					Response::Amount(0)
				}
				_ => Response::Error(Error::InvalidOperation),
			};
			table.enqueue(job_id, response);
			send_notif = true;
		}
		send_notif.then(|| table.flush());

		if let Some(new) = draw_focus_borders {
			for (r, c) in old
				.map(|o| (o, [50; 3]))
				.into_iter()
				.chain([(new, [127; 3])])
			{
				let w = config.margin;
				let (l, h) = (r.low() - Vector::ONE * w, r.high() + Vector::ONE);
				let s = Size::new(r.size().x + w * 2, r.size().y + w * 2);
				for r in [
					Rect::from_size(Point::new(l.x, l.y), Size::new(w, s.y)),
					Rect::from_size(Point::new(h.x, l.y), Size::new(w, s.y)),
					Rect::from_size(Point::new(l.x, l.y), Size::new(s.x, w)),
					Rect::from_size(Point::new(l.x, h.y), Size::new(s.x, w)),
				] {
					main.fill(r, c);
				}
			}
			old = Some(new);
		}
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
		Self { base: NonNull::dangling(), size: 0 }
	}
}

#[derive(Default)]
struct Client {
	framebuffer: FrameBuffer,
	unread_events: RefCell<Events>,
	event_listeners: RefCell<VecDeque<JobId>>,
	title: Box<str>,
}

#[derive(Default)]
struct Events {
	resize: Option<ipc_wm::Resolution>,
	close: bool,
	inputs: VecDeque<input::Input>,
}

impl Events {
	fn pop(&mut self) -> Option<ipc_wm::Event> {
		if core::mem::take(&mut self.close) {
			return Some(ipc_wm::Event::Close);
		}
		self.resize
			.take()
			.map(ipc_wm::Event::Resize)
			.or_else(|| self.inputs.pop_front().map(ipc_wm::Event::Input))
	}
}
