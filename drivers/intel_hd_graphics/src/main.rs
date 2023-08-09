//! # Intel HD Graphics driver
//!
//! Based on https://github.com/managarm/managarm/blob/master/drivers/gfx/intel/ and
//! https://github.com/himanshugoel2797/Cardinal/tree/master/drivers/display/ihd/common
//!
//! Documentation can be found at https://01.org/linuxgraphics/documentation
//!
//! (Incomplete) guide can be foudn at https://wiki.osdev.org/Intel_HD_Graphics
//!
//! ## Supported devices
//!
//! - HD Graphics 5500

#![no_std]
#![feature(alloc_error_handler)]
#![feature(core_intrinsics)]
#![feature(start)]
#![feature(inline_const)]
#![feature(slice_as_chunks)]
// FIXME clean this crate up
#![allow(dead_code, unreachable_code, unused_variables, unused_mut)]

extern crate alloc;

macro_rules! reg {
	(@INTERNAL $fn:ident $setfn:ident [$bit:literal] $ty:ty) => {
		#[allow(dead_code)]
		pub fn $fn(&self) -> $ty {
			self.0 & const { 1 << $bit } != 0
		}

		#[allow(dead_code)]
		pub fn $setfn(&mut self, enable: $ty) -> &mut Self {
			self.0 &= const { !(1 << $bit) };
			self.0 |= u32::from(enable) << $bit;
			self
		}
	};
	(@INTERNAL $fn:ident $setfn:ident [(try $high:literal:$low:literal)] $ty:ty) => {
		#[allow(dead_code)]
		pub fn $fn(&self) -> Option<$ty> {
			const MASK: u32 = (1 << ($high - $low + 1)) - 1;
			<$ty>::try_from((self.0 >> $low) & MASK).ok()
		}

		#[allow(dead_code)]
		pub fn $setfn(&mut self, value: $ty) -> &mut Self {
			const MASK: u32 = (1 << ($high - $low + 1)) - 1;
			self.0 &= const { !(MASK << $low) };
			self.0 |= (value as u32) << $low;
			self
		}
	};
	(@INTERNAL $fn:ident $setfn:ident [($high:literal:$low:literal)] $ty:ty) => {
		#[allow(dead_code)]
		#[track_caller]
		pub fn $fn(&self) -> $ty {
			const MASK: u32 = (1 << ($high - $low + 1)) - 1;
			use $crate::PanicFrom;
			<$ty>::panic_from((self.0 >> $low) & MASK)
		}

		#[allow(dead_code)]
		pub fn $setfn(&mut self, value: $ty) -> &mut Self {
			const MASK: u32 = (1 << ($high - $low + 1)) - 1;
			self.0 &= const { !(MASK << $low) };
			self.0 |= u32::from(value) << $low;
			self
		}
	};
	{
		$(#[doc = $doc:literal])*
		$name:ident @ $address:literal
		$($fn:ident $setfn:ident [$param:tt] $ty:ty)*
	} => {
		$(#[doc = $doc])*
		#[allow(dead_code)]
		pub struct $name(u32);

		impl $name {
			#[allow(dead_code)]
			pub const REG: u32 = $address;

			#[allow(dead_code)]
			pub fn from_raw(n: u32) -> Self {
				Self(n)
			}

			#[allow(dead_code)]
			pub fn as_raw(&self) -> u32 {
				self.0
			}

			$(reg!(@INTERNAL $fn $setfn [$param] $ty);)*
		}
	};
	{
		$(#[doc = $doc:literal])*
		$name:ident
		$($fn:ident $setfn:ident [$param:tt] $ty:ty)*
	} => {
		$(#[doc = $doc])*
		#[allow(dead_code)]
		#[derive(Clone)]
		pub struct $name(u32);

		impl $name {
			#[allow(dead_code)]
			pub fn from_raw(n: u32) -> Self {
				Self(n)
			}

			#[allow(dead_code)]
			pub fn as_raw(&self) -> u32 {
				self.0
			}

			$(reg!(@INTERNAL $fn $setfn [$param] $ty);)*
		}
	};
}

trait PanicFrom<T> {
	fn panic_from(t: T) -> Self;
}

impl PanicFrom<u32> for u8 {
	fn panic_from(t: u32) -> u8 {
		t.try_into().unwrap()
	}
}

impl PanicFrom<u32> for u16 {
	#[track_caller]
	fn panic_from(t: u32) -> u16 {
		t.try_into().unwrap()
	}
}

macro_rules! bit2enum {
	{
		$name:ident
		$($variant:ident $val:literal)*
	} => {
		#[derive(Clone, Copy, Debug, PartialEq)]
		pub enum $name {
			$($variant = $val,)*
		}

		impl $crate::PanicFrom<u32> for $name {
			fn panic_from(value: u32) -> Self {
				match value {
					$($val => Self::$variant,)*
					_ => unreachable!(),
				}
			}
		}

		impl From<$name> for u32 {
			fn from(s: $name) -> Self {
				s as Self
			}
		}
	};
	{
		try $name:ident
		$($variant:ident $val:literal)*
	} => {
		#[derive(Clone, Copy, Debug, PartialEq)]
		pub enum $name {
			$($variant = $val,)*
		}

		impl TryFrom<u32> for $name {
			type Error = ();

			fn try_from(value: u32) -> Result<Self, Self::Error> {
				match value {
					$($val => Ok(Self::$variant),)*
					_ => Err(()),
				}
			}
		}

		impl From<$name> for u32 {
			fn from(s: $name) -> Self {
				s as Self
			}
		}
	};
}

macro_rules! impl_reg {
	($base:literal $val:ident $load:ident $store:ident) => {
		unsafe fn $load(&self, control: &mut Control) -> $val {
			$val(control.load($base + self.offset()))
		}

		unsafe fn $store(&self, control: &mut Control, val: $val) {
			control.store($base + self.offset(), val.0)
		}
	};
}

macro_rules! log {
	($($arg:tt)*) => {{
		let _ = rt::io::stderr().map(|o| writeln!(o, $($arg)*));
	}};
}

#[derive(Clone, Copy)]
pub struct GraphicsAddress(u32);

impl PanicFrom<u32> for GraphicsAddress {
	fn panic_from(n: u32) -> Self {
		assert_eq!(n & 0xfff, 0, "address is not aligned");
		Self(n)
	}
}

impl From<GraphicsAddress> for u32 {
	fn from(addr: GraphicsAddress) -> Self {
		addr.0
	}
}

mod backlight;
mod console;
mod control;
mod ddi;
mod displayport;
mod edid;
mod gmbus;
mod mode;
mod panel;
mod pipe;
mod plane;
mod pll;
mod transcoder;
mod vga;
mod watermark;

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64;
use {
	core::ptr::NonNull,
	driver_utils::os::stream_table::{Request, Response, StreamTable},
	rt::{Error, Handle},
};

#[global_allocator]
static ALLOC: rt_alloc::Allocator = rt_alloc::Allocator;

#[alloc_error_handler]
fn alloc_error(_: core::alloc::Layout) -> ! {
	// FIXME the runtime allocates memory by default to write things, so... crap
	// We can run in similar trouble with the I/O queue. Some way to submit I/O requests
	// without going through an queue may be useful.
	rt::exit(129)
}

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
	let _ = rt::io::stderr().map(|o| writeln!(o, "{}", info));
	rt::exit(128)
}

#[derive(Debug)]
enum Model {
	HD5500,
}

impl Model {
	fn try_from_pci_id(vendor_id: u16, device_id: u16) -> Option<Self> {
		Some(match (vendor_id, device_id) {
			(0x8086, 0x1616) => Self::HD5500,
			_ => return None,
		})
	}
}

#[start]
fn main(_: isize, _: *const *const u8) -> isize {
	let root = rt::io::file_root().unwrap();

	// Open & configure
	let dev = rt::args::handles()
		.find(|(name, _)| name == b"pci")
		.expect("no 'pci' object")
		.1;
	let ioport = root.open(b"portio/map").expect("can't access I/O ports");

	let (pci_config, _) = dev.map_object(None, rt::RWX::R, 0, usize::MAX).unwrap();

	let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

	let (width, height);
	let mut display_fb;
	{
		let h = pci.get(0, 0, 0).unwrap();
		log!("{:?}", h);
		match h {
			pci::Header::H0(h) => {
				let map_bar = |bar: u8| {
					assert!(bar < 6);
					let mut s = *b"bar0";
					s[3] += bar;
					dev.open(&s)
						.unwrap()
						.map_object(None, rt::io::RWX::RW, 0, usize::MAX)
						.unwrap()
						.0
				};

				let control = map_bar(0);
				let memory = map_bar(2);

				let mut control = control::Control::new(control.cast());

				// This is the only errata I found wrt. GMBUS. (see vol15) and DP AUX
				// It doesn't seem to do anything though.
				unsafe {
					let v = control.load(0xc2020);
					control.store(0xc2020, v | (1 << 12));
				}

				// Ensure Self Refreshing Display is disabled (SRD)
				const SRD_CTL_A: u32 = 0x60800;
				const SRD_CTL_B: u32 = 0x61800;
				const SRD_CTL_C: u32 = 0x62800;
				const SRD_CTL_EDP: u32 = 0x6f800;
				for reg in [SRD_CTL_A, SRD_CTL_B, SRD_CTL_C, SRD_CTL_EDP] {
					unsafe {
						let v = control.load(reg);
						control.store(reg, v & !(1 << 31));
					}
				}

				let mut edid = [0; 128];
				let mut port = None;
				let port = unsafe {
					for p in [
						displayport::Port::A,
						displayport::Port::B,
						displayport::Port::C,
						displayport::Port::D,
					] {
						match displayport::i2c_write_read(&mut control, p, 0x50, &[0], &mut edid) {
							Ok(()) => {
								for c in edid.as_chunks::<16>().0 {
									log!("{:02x?}", c);
								}
								port = Some(p);
								break;
							}
							Err(e) => log!("{:?}", e),
						}
					}
					if let Some(port) = port {
						port
					} else {
						log!("No DisplayPort device found");
						return 1;
					}
				};
				assert_eq!(port, displayport::Port::A, "TODO support multiple ports");
				let edid = edid::Edid::new(edid).unwrap();
				let mode = mode::Mode::from_edid(&edid).unwrap();

				pll::compute_sdvo(mode.pixel_clock);

				(width, height) = (mode.horizontal.active + 1, mode.vertical.active + 1);

				let stride = width * 4;
				let stride = (stride + 63) & !63;
				let config = plane::Config {
					base: GraphicsAddress(0),
					// FIXME this doesn't work.
					//format: plane::PixelFormat::RGBX8888,
					format: plane::PixelFormat::BGRX8888,
					stride,
				};

				unsafe {
					for loc in [
						0x70180, // PRI_CTL_A
						0x70188, // PRI_STRIDE_A
						0x68080, // PF_CTRL_A
						0x68074, // PF_WIN_SZ_A
						0x6001C, // PIPE_SRCSZ_A
						0x46100, // PORT_CLK_SEL_DDIA
						0x41000, // VGA_CONTROL
					] {
						log!("{:08x} -> {:08x}", loc, control.load(loc));
					}
				}

				// See vol11 p. 112 "Sequences for DisplayPort"
				// FIXME configure PLL ourselves instead of relying on preset value.
				use transcoder::Transcoder;
				unsafe {
					// Disable sequence
					// b. Disable planes (VGA or hires)
					vga::disable_vga(&mut control, (&ioport).into());
					plane::disable(&mut control, plane::Plane::A);
					// c. Disable TRANS_CONF
					transcoder::disable(&mut control, Transcoder::EDP);
					// h. Disable panel fitter
					panel::disable_fitter(&mut control, panel::Pipe::A);
					// i. Configure Transcoder Clock Select to direct no clock to the transcoder
					transcoder::disable_clock(&mut control, Transcoder::EDP);
					displayport::disable(&mut control, displayport::Port::A);
					backlight::disable(&mut control);
					displayport::set_port_clock(
						&mut control,
						displayport::Port::A,
						displayport::PortClock::None,
					);

					//pipe::configure(&mut control, pipe::Pipe::A, &mode);

					// FIXME don't hardcode port clock, configure it properly instead
					backlight::enable_panel(&mut control);
					displayport::configure(
						&mut control,
						displayport::Port::A,
						displayport::PortClock::LcPll1350,
					);
					// a. If DisplayPort multi-stream - use AUX to program receiver VC Payload ID
					// table to add stream

					// b. Configure Transcoder Clock Select to direct the Port clock to the
					// Transcoder
					transcoder::configure_clock(&mut control, Transcoder::EDP, None);
					// c. Configure and enable planes (VGA or hires). This can be done later if
					// desired.
					pipe::configure(&mut control, pipe::Pipe::A, &mode);
					plane::enable(&mut control, plane::Plane::A, config);
					transcoder::configure_rest(&mut control, Transcoder::EDP, None, mode);
					//transcoder::enable_only(&mut control, Transcoder::EDP);
					// k. If eDP (DDI A), set DP_TP_CTL link training to Normal
					displayport::set_training_pattern(
						&mut control,
						displayport::Port::A,
						displayport::LinkTraining::Normal,
					);
					backlight::enable_backlight(&mut control);

					let v = control.load(SRD_CTL_EDP);
					control.store(SRD_CTL_EDP, v | (1 << 31));
				}

				let plane_buf = memory.cast::<[u8; 4]>();
				unsafe {
					let (x, mut y) = (20, 80);
					let real_stride =
						usize::from(plane::get_stride(&mut control, plane::Plane::A) / 4);
					for loc in [
						0x70180, // PRI_CTL_A
						0x70188, // PRI_STRIDE_A
						0x68080, // PF_CTRL_A
						0x68074, // PF_WIN_SZ_A
						0x6001C, // PIPE_SRCSZ_A
						0x43408, // IPS_CTL
						0x45270, // WM_LINETIME_A
						0x46100, // PORT_CLK_SEL_DDIA
					] {
						let b = console::bitify_u32_hex(control.load(loc));
						//let b = console::bitify_u32_hex(loc);
						for (dy, l) in b.iter().enumerate() {
							for dx in 0..8 * 8 {
								let on = l[dx / 8] & (1 << 7 - dx % 8) != 0;
								let clr = [[0; 4], [255; 4]][usize::from(on)];
								*plane_buf.as_ptr().add((y + dy) * real_stride + x + dx) = clr;
							}
						}
						y += 12;
					}
				}

				// Funny colors GO
				for y in 0..height {
					let stride = usize::from(stride) / 4;
					for x in 0..width {
						let (x, y) = (usize::from(x), usize::from(y));
						let r = x * 256 / usize::from(width);
						let g = y * 256 / usize::from(height);
						let b = 255 - (r + g) / 2;
						let bgrx = [b as u8, g as u8, r as u8, 0];
						unsafe {
							x86_64::_mm_stream_si32(
								memory.cast::<i32>().as_ptr().add(y * stride + x),
								i32::from_ne_bytes(bgrx),
							);
						}
					}
				}

				// Used nontemporal stores in the immediately preceding loop, so end with SFENCE
				// See safety comment on DisplayFrameBuffer::stream_untrusted_row_rgb24_to_bgrx32
				x86_64::_mm_sfence();

				let base = memory.cast().try_into().unwrap();
				let stride = stride / 4;
				display_fb = DisplayFrameBuffer { base, width, height, stride };
			}
			_ => unreachable!(),
		}
	};

	let table = {
		let (buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 12 }).unwrap();
		let tbl = StreamTable::new(&buf, rt::io::Pow2Size(5), (1 << 8) - 1);
		root.create(b"gpu").unwrap().share(tbl.public()).unwrap();
		tbl
	};

	const SYNC_HANDLE: Handle = Handle::MAX - 1;

	let mut command_buf = (NonNull::<u8>::dangling(), 0);

	let mut tiny_buf = [0; 511];
	loop {
		let mut flush = false;
		while let Some((handle, job_id, req)) = table.dequeue() {
			let resp = match req {
				Request::Open { path } => {
					let mut p = [0; 64];
					let p = &mut p[..path.len()];
					path.copy_to(0, p);
					match (handle, &*p) {
						(Handle::MAX, b"sync") => Response::Handle(SYNC_HANDLE),
						(Handle::MAX, _) => Response::Error(Error::DoesNotExist as _),
						_ => Response::Error(Error::InvalidOperation as _),
					}
				}
				Request::GetMeta { property } => {
					let prop = property.get(&mut tiny_buf);
					match (handle, &*prop) {
						(_, b"bin/resolution") => {
							let r = ipc_gpu::Resolution { x: width as _, y: height as _ }.encode();
							let data = table.alloc(r.len()).unwrap();
							data.copy_from(0, &r);
							Response::Data(data)
						}
						_ => Response::Error(Error::DoesNotExist),
					}
				}
				Request::SetMeta { property_value } => {
					Response::Error(Error::InvalidOperation as _)
				}
				Request::Write { data } => {
					let mut d = [0; 64];
					let d = &mut d[..data.len()];
					data.copy_to(0, d);
					match handle {
						// Blit a specific area
						SYNC_HANDLE => {
							if let Ok(d) = d.try_into() {
								let cmd = ipc_gpu::Flush::decode(d);
								unsafe {
									display_fb.copy_from_raw_untrusted_rgb24_to_bgrx32(
										command_buf.0.as_ptr().add(cmd.offset as _).cast(),
										cmd.stride as _,
										cmd.origin.x as _,
										cmd.origin.y as _,
										cmd.size.x,
										cmd.size.y,
									);
								}
								Response::Amount(d.len().try_into().unwrap())
							} else {
								Response::Error(Error::InvalidData as _)
							}
						}
						_ => Response::Error(Error::InvalidOperation as _),
					}
				}
				Request::Share { share } => match handle {
					SYNC_HANDLE => match share.map_object(None, rt::io::RWX::R, 0, 1 << 30) {
						Err(e) => Response::Error(e as _),
						Ok((buf, size)) => {
							command_buf = (buf, size);
							Response::Amount(0)
						}
					},
					_ => Response::Error(Error::InvalidOperation as _),
				},
				Request::Close => match handle {
					Handle::MAX | SYNC_HANDLE => continue,
					_ => unreachable!(),
				},
				Request::Create { path } => (Response::Error(Error::InvalidOperation as _)),
				Request::Read { .. } | Request::Destroy { .. } | Request::Seek { .. } => {
					Response::Error(Error::InvalidOperation as _)
				}
			};
			flush = true;
			table.enqueue(job_id, resp);
		}
		flush.then(|| table.flush());
		table.wait();
	}
}

struct DisplayFrameBuffer {
	base: NonNull<i32>,
	width: u16,
	height: u16,
	stride: u16,
}

impl DisplayFrameBuffer {
	unsafe fn copy_from_raw_untrusted_rgb24_to_bgrx32(
		&mut self,
		mut src: *const [u8; 3],
		stride: u16,
		x: u16,
		y: u16,
		w: u16,
		h: u16,
	) {
		if w == 0 || h == 0 {
			return;
		}
		let f = usize::from;
		let (stride, x, y, w, h) = (f(stride), f(x), f(y), f(w), f(h));
		assert!(x < f(self.width) && x + w <= f(self.width));
		assert!(y < f(self.height) && y + h <= f(self.height));
		let mut dst = self.base.as_ptr().add(x).add(y * f(self.stride));
		let pre_end = dst.add((h - 1) * f(self.stride));
		let end = pre_end.add(f(self.stride));
		while dst != pre_end {
			Self::stream_untrusted_row_rgb24_to_bgrx32(dst, src, w, false);
			src = src.add(stride);
			dst = dst.add(f(self.stride));
		}
		Self::stream_untrusted_row_rgb24_to_bgrx32(dst, src, w, true);
		// Required before returning to code that may set atomic flags that invite concurrent reads,
		// as LLVM lowers `AtomicBool::store(flag, true, Release)` to ordinary stores on x86-64
		// instead of SFENCE, even though SFENCE is required in the presence of nontemporal stores.
		x86_64::_mm_sfence();
	}

	/// Translate a row with streaming stores
	///
	/// # Safety
	///
	/// This is a form of nontemporal store, and imposes the requirement of inserting SFENCE
	/// via `_mm_sfence` or `asm!` in order to maintain the soundness of atomic barriers.
	/// Control flow should not leave an `unsafe` context before inserting this SFENCE, and
	/// it must be inserted before atomic operations are reached (e.g. `AtomicBool::store`).
	#[inline]
	unsafe fn stream_untrusted_row_rgb24_to_bgrx32(
		mut dst: *mut i32,
		mut src: *const [u8; 3],
		w: usize,
		last: bool,
	) {
		#[repr(C)]
		struct E(u16, u8);
		let end = dst.add(w);
		// Special-case w <= 4 so the much more common loop is simpler & faster
		if w <= 4 {
			while dst != end {
				let E(a, c) = read_unaligned_untrusted(src.cast::<E>());
				let [a, b] = a.to_le_bytes();
				x86_64::_mm_stream_si32(dst, i32::from_le_bytes([c, b, a, 0]));
				src = src.add(1);
				dst = dst.add(1);
			}
		} else {
			// Align 16
			while dst as usize & 0b1111 != 0 {
				let v = read_unaligned_untrusted(src.cast::<i32>());
				x86_64::_mm_stream_si32(dst, v.to_be() >> 8);
				src = src.add(1);
				dst = dst.add(1);
			}
			// Loop 16
			let mut end_16 = (end as usize & !0b1111) as *mut i32;
			// Be careful with out of bounds reads
			if last && end_16 == end {
				end_16 = end_16.sub(4);
			}
			let shuf = x86_64::_mm_set_epi8(-1, 9, 10, 11, -1, 6, 7, 8, -1, 3, 4, 5, -1, 0, 1, 2);
			while dst != end_16 {
				let v = read_unaligned_untrusted(src.cast::<x86_64::__m128i>());
				let v = x86_64::_mm_shuffle_epi8(v, shuf);
				x86_64::_mm_stream_si128(dst.cast(), v);
				src = src.add(4);
				dst = dst.add(4);
			}
			// Copy remaining bytes (up to 16)
			// Be careful again
			while dst != end {
				let E(a, c) = read_unaligned_untrusted(src.cast::<E>());
				let [a, b] = a.to_le_bytes();
				x86_64::_mm_stream_si32(dst, i32::from_le_bytes([c, b, a, 0]));
				src = src.add(1);
				dst = dst.add(1);
			}
		}
	}
}

unsafe fn read_unaligned_untrusted<T>(ptr: *const T) -> T {
	core::intrinsics::unaligned_volatile_load(ptr)
}
