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
mod gpu;
#[macro_use]
mod manager;
mod title_bar;
mod window;
mod workspace;

use {
	core::{cell::RefCell, ptr::NonNull, time::Duration},
	driver_utils::{
		os::stream_table::{JobId, Request, Response, StreamTable},
		task,
	},
	gui3d::math::int as math,
	io_queue_rt::{Pow2Size, Queue},
	math::{Point2, Rect, Size, Vec2},
	rt::io::{Error, Handle},
	std::collections::VecDeque,
};

fn main() {
	let config = config::load();

	let mut mgr = manager::Manager::new().unwrap();

	let mut main = gpu::Gpu::new();

	let (tbl_buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 12 }).unwrap();
	let table = StreamTable::new(&tbl_buf, rt::io::Pow2Size(5), (1 << 8) - 1);
	rt::args::handle(b"share")
		.expect("share undefined")
		.share(table.public())
		.expect("failed to share");

	main.set_cursor(&config.cursor);

	let mut mouse_pos = Point2::new((main.size().x / 2).into(), (main.size().y / 2).into());
	main.move_cursor(mouse_pos);

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

		let size_x2 = Size::new(
			(main.size().x - config.margin) * 2,
			(main.size().y - config.margin) * 2,
		);
		let unsize_x2 = |r: Rect| {
			let m = config.margin;
			let l = Point2::new((r.low().x + m) / 2, (r.low().y + m) / 2);
			let h = Point2::new((r.high().x + m) / 2, (r.high().y + m) / 2);
			Rect::from_points(l, h)
		};
		let apply_margin = |r: Rect| {
			let l = r.low() + Vec2::ONE * config.margin;
			let h = r.high() - Vec2::ONE * config.margin;
			Rect::from_points(l, h)
		};
		let window_rect = |mgr: &manager::Manager, h| {
			let r = mgr.window_rect(h, size_x2).unwrap();
			let r = apply_margin(r);
			unsize_x2(r)
		};
		let window_at = |mgr: &mut manager::Manager, pos: Point2| {
			let pos = Point2::new(pos.x * 2 - config.margin, pos.y * 2 - config.margin);
			let (h, r) = mgr.window_at(pos, size_x2).unwrap();
			if Some(h) != mgr.focused_window() {
				mgr.set_focused_window(h);
				let r = apply_margin(r);
				let r = unsize_x2(r);
				Some(r)
			} else {
				None
			}
		};

		while let Some((handle, job_id, req)) = table.dequeue() {
			let mut prop_buf = [0; 511];
			let response = match req {
				Request::Create { path } => {
					let mut p = [0; 8];
					let (p, _) = path.copy_into(&mut p);
					match (handle, &*p) {
						(Handle::MAX, b"window") => {
							let h = mgr.new_window(main.size()).unwrap();
							main.fill(Rect::from_size(Point2::ORIGIN, main.size()), [50; 3]);
							old = None;
							for w in mgr!(mgr, current_workspace).windows() {
								let full_rect = window_rect(&mgr, w);
								let ww = &mut mgr.windows[w];
								let (title, rect) = title_bar::split(&config, full_rect);
								title_bar::render(&mut main, &config, title, mouse_pos, &ww.title);
								let evt = ipc_wm::Resolution { x: rect.size().x, y: rect.size().y };
								ww.unread_events.resize = Some(evt);
								let evt = ipc_wm::Event::Resize(evt).encode();
								for id in ww.event_listeners.drain(..) {
									ww.unread_events.resize = None;
									let data = table.alloc(evt.len()).expect("out of buffers");
									data.copy_from(0, &evt);
									table.enqueue(id, Response::Data(data));
								}
								if Some(w) == mgr.focused_window() {
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
							let rect = window_rect(&mgr, h);
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
								let rect = window_rect(&mgr, h);
								let (_, rect) = title_bar::split(&config, rect);
								main.fill(rect, [r, g, b]);
								Response::Amount(0)
							} else {
								Response::Error(Error::InvalidData)
							}
						}
						(h, b"title") => {
							let s = String::from_utf8_lossy(val).into_owned().into_boxed_str();
							let r = window_rect(&mgr, h);
							let (r, _) = title_bar::split(&config, r);
							title_bar::render(&mut main, &config, r, mouse_pos, &s);
							mgr.window_mut(h).unwrap().title = s;
							Response::Amount(0)
						}
						(_, _) => Response::Error(Error::DoesNotExist as _),
					}
				}
				Request::Read { amount: _ } if handle != Handle::MAX => {
					let w = &mut mgr.window_mut(handle).unwrap();
					if let Some(evt) = w.unread_events.pop() {
						let evt = evt.encode();
						let data = table.alloc(evt.len()).expect("out of buffers");
						data.copy_from(0, &evt);
						Response::Data(data)
					} else {
						w.event_listeners.push_back(job_id);
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
										(mouse_pos.x + l as u32).min(main.size().x - 1)
									} else {
										mouse_pos.x.saturating_sub(-l as u32)
									};
									mouse_moved = true;
								}
								Type::Relative(0, Movement::TranslationY) => {
									mouse_pos.y = if l >= 0 {
										mouse_pos.y.saturating_sub(l as u32)
									} else {
										(mouse_pos.y + -l as u32).min(main.size().y - 1)
									};
									mouse_moved = true;
								}
								Type::Absolute(0, Movement::TranslationX) => {
									mouse_pos.x =
										(l as u64 * main.size().x as u64 / (1 << 31)) as _;
									mouse_moved = true;
								}
								Type::Absolute(0, Movement::TranslationY) => {
									mouse_pos.y =
										(l as u64 * main.size().y as u64 / (1 << 31)) as _;
									mouse_moved = true;
								}
								Type::Button(0) => mouse_clicked = k.is_press(),
								_ => {
									let Some(w) = mgr.focused_window() else { continue };
									let u = &mut mgr.window_mut(w).unwrap();
									if let Some(id) = u.event_listeners.pop_front() {
										let evt = ipc_wm::Event::Input(k).encode();
										let d = table.alloc(evt.len()).expect("out of buffers");
										d.copy_from(0, &evt);
										table.enqueue(id, Response::Data(d));
									} else {
										u.unread_events.inputs.push_back(k);
									}
								}
							};
						}
					}
					let edge = !mouse_was_clicked & mouse_clicked;
					if mouse_moved {
						main.move_cursor(mouse_pos);
					}
					if mouse_moved | edge {
						for w in mgr!(mgr, current_workspace).windows() {
							let full_rect = window_rect(&mgr, w);
							let ww = &mut mgr.windows[w];
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
								if let Some(id) = ww.event_listeners.pop_front() {
									let evt = ipc_wm::Event::Close.encode();
									let d = table.alloc(evt.len()).expect("out of buffers");
									d.copy_from(0, &evt);
									table.enqueue(id, Response::Data(d));
								} else {
									ww.unread_events.close = true;
								}
							}
						}
					}
					if edge {
						if let Some(r) = window_at(&mut mgr, mouse_pos) {
							draw_focus_borders = Some(r);
						}
					}
					Response::Amount(data.len() as _)
				}
				Request::Write { data } if handle != Handle::MAX => {
					let window = mgr.window(handle).unwrap();
					let mut header = [0; 12];
					data.copy_to(0, &mut header);
					let rect = window_rect(&mgr, handle);
					let (_, rect) = title_bar::split(&config, rect);
					let draw = ipc_wm::Flush::decode(header);
					let draw_size = draw.size;
					// TODO do we actually want this?
					let draw_size = Size::new(
						(u32::from(draw_size.x) + 1).min(rect.size().x),
						(u32::from(draw_size.y) + 1).min(rect.size().y),
					);
					let draw_orig = draw.origin;
					let draw_orig = Point2::new(draw_orig.x, draw_orig.y);
					let draw_rect = rect
						.calc_global_pos(Rect::from_size(draw_orig, draw_size))
						.unwrap();
					main.sync_rect(Some(window.framebuffer), draw_rect);
					Response::Amount(data.len() as _)
				}
				Request::Close if handle != INPUT => {
					let w = mgr.destroy_window(handle).unwrap();
					if w.framebuffer != u32::MAX {
						main.unmap_buffer(w.framebuffer).unwrap();
					}
					main.fill(Rect::from_size(Point2::ORIGIN, main.size()), [50, 50, 50]);
					old = None;
					for w in mgr!(mgr, current_workspace).windows() {
						let full_rect = window_rect(&mgr, w);
						let ww = &mut mgr.windows[w];
						let (title, rect) = title_bar::split(&config, full_rect);
						title_bar::render(&mut main, &config, title, mouse_pos, &ww.title);
						let evt = ipc_wm::Resolution { x: rect.size().x, y: rect.size().y };
						ww.unread_events.resize = Some(evt);
						let evt = ipc_wm::Event::Resize(evt).encode();
						for id in ww.event_listeners.drain(..) {
							ww.unread_events.resize = None;
							let data = table.alloc(evt.len()).expect("out of buffers");
							data.copy_from(0, &evt);
							table.enqueue(id, Response::Data(data));
							send_notif = true;
						}
						if Some(w) == mgr.focused_window() {
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
					match main.share_buffer(share) {
						Ok(h) => {
							mgr.window_mut(handle).unwrap().framebuffer = h;
							Response::Amount(0)
						}
						Err(e) => Response::Error(e),
					}
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
				let (l, h) = (r.low() - Vec2::ONE * w, r.high() + Vec2::ONE);
				let s = Size::new(r.size().x + w * 2, r.size().y + w * 2);
				for r in [
					Rect::from_size(Point2::new(l.x, l.y), Size::new(w, s.y)),
					Rect::from_size(Point2::new(h.x, l.y), Size::new(w, s.y)),
					Rect::from_size(Point2::new(l.x, l.y), Size::new(s.x, w)),
					Rect::from_size(Point2::new(l.x, h.y), Size::new(s.x, w)),
				] {
					main.fill(r, c);
				}
			}
			old = Some(new);
		}
	}
}

#[derive(Default)]
pub struct Events {
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
