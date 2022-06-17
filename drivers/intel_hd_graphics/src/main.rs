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
#![feature(start)]
#![feature(inline_const)]
#![feature(slice_as_chunks)]

extern crate alloc;

use core::time::Duration;

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

use alloc::vec::Vec;

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

	// Find suitable device
	let (path, model) = {
		let it = root.open(b"pci/info").unwrap();
		loop {
			let mut e = it.read_vec(32).unwrap();
			if e.is_empty() {
				log!("no suitable Intel HD Graphics device found");
				return 1;
			}
			let s = core::str::from_utf8(&e).unwrap();
			let (loc, id) = s.split_once(' ').unwrap();
			let (v, d) = id.split_once(':').unwrap();
			let f = |s| u16::from_str_radix(s, 16).unwrap();
			if let Some(model) = Model::try_from_pci_id(f(v), f(d)) {
				e.resize(loc.len(), 0);
				e.splice(0..0, *b"pci/");
				break (e, model);
			}
		}
	};
	log!("{:?}", (&path, model));

	// Open & configure
	let dev = root.open(&path).unwrap();

	let pci_config = kernel::syscall::map_object(dev.as_raw(), None, 0, usize::MAX).unwrap();

	let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

	{
		let h = pci.get(0, 0, 0).unwrap();
		log!("{:?}", h);
		match h {
			pci::Header::H0(h) => {
				let map_bar = |bar: u8| {
					kernel::syscall::map_object(dev.as_raw(), None, (bar + 1).into(), usize::MAX)
						.unwrap()
				};
				/*
				let dma_alloc = |size, _align| -> Result<_, ()> {
					let (d, _) = syscall::alloc_dma(None, size).unwrap();
					let a = syscall::physical_address(d).unwrap();
					Ok((d.cast(), virtio::PhysAddr::new(a.try_into().unwrap())))
				};
				*/

				log!("a");
				let control = map_bar(0);
				log!("b");
				let memory = map_bar(2);
				log!("c {:?}", (control, memory));

				/*
				rt::thread::sleep(core::time::Duration::from_secs(5));
				unsafe {
					let b = base.as_ptr().cast::<u64>();
					for i in 0..10 {
						*b.add(i) = u64::MAX;
						log!("{}", i);
						rt::thread::sleep(core::time::Duration::from_secs(5));
					}
				}
				*/

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
				let edid = edid::Edid::new(edid).unwrap();
				let mode = mode::Mode::from_edid(&edid).unwrap();
				log!("{:?}", &mode);

				let vga_enable = root.open(b"vga/enable").unwrap();
				log!("{:?}", vga_enable.read_vec(1).unwrap());
				//rt::thread::sleep(Duration::MAX);

				pll::compute_sdvo(mode.pixel_clock);

				//log!("Disabling VGA, enabling primary surface & painting colors in 3 seconds");
				//rt::thread::sleep(Duration::from_secs(3));

				let (width, height) = (mode.horizontal.active + 1, mode.vertical.active + 1);

				let stride = width * 4;
				let stride = (stride + 63) & !63;
				let config = plane::Config {
					base: GraphicsAddress(0),
					format: plane::PixelFormat::BGRX8888,
					stride,
				};

				// See vol11 p. 112 "Sequences for DisplayPort"
				// FIXME configure PLL ourselves instead of relying on preset value.
				use transcoder::Transcoder;
				unsafe {
					// Disable sequence
					// b. Disable planes (VGA or hires)
					//plane::disable(&mut control, plane::Plane::A);
					vga_enable.write(&[0]).unwrap();
					rt::thread::sleep(Duration::from_micros(100));
					vga::disable_vga(&mut control);
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

					/*
					let v = control.load(SRD_CTL_EDP);
					control.store(SRD_CTL_EDP, v | (1 << 31));
					*/
				}

				/*
				unsafe {
					vga_enable.write(&[0]).unwrap();
					vga::disable_vga(&mut control);
					// This is the most minimal sequence that kinda-but-not-really works
					plane::disable(&mut control, plane::Plane::A);
					rt::thread::sleep(Duration::from_millis(1));
					pipe::set_hv(&mut control, pipe::Pipe::A, 1919, 1079);
					panel::set_hv(&mut control, panel::Pipe::A, 1919, 1080);
					//panel::disable_fitter(&mut control, panel::Pipe::A);
					//panel::enable_fitter(&mut control, panel::Pipe::A);
					plane::enable(&mut control, plane::Plane::A, config);
				}
				*/

				let plane_buf = memory.cast::<[u8; 4]>();
				unsafe {
					let (x, mut y) = (20, 80);
					let real_stride =
						usize::from(plane::get_stride(&mut control, plane::Plane::A) / 4);
					//let real_stride = 0x1000 / 4;
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

				//rt::thread::sleep(Duration::MAX);

				// Funny colors GO
				let stride = 1920;
				let plane_buf = memory.cast::<[u8; 4]>();
				for y in 0..height {
					for x in 0..width {
						let (x, y) = (usize::from(x), usize::from(y));
						let r = x * 256 / usize::from(width);
						let g = y * 256 / usize::from(height);
						let b = 255 - (r + g) / 2;
						let bgrx = [b as u8, g as u8, r as u8, 0];
						//let bgrx = [255 - r as u8, ((y % 4) * 64) as u8, r as u8, 0];
						unsafe {
							*plane_buf.as_ptr().add(y * stride + x) = bgrx;
							//*plane_buf.as_ptr().add(y * usize::from(width) + x) = 0x00ff0000;
						}
						//rt::thread::sleep(Duration::from_millis(1));
					}
				}
			}
			_ => unreachable!(),
		}
	};

	log!("stop");
	rt::thread::sleep(core::time::Duration::MAX);
	0
}
