//! # Intel HD Graphics driver
//!
//! Based on https://github.com/managarm/managarm/blob/master/drivers/gfx/intel/
//! Documentation can be found at https://01.org/linuxgraphics/documentation
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
			self.0 |= (value as u32) << $low;
			self
		}
	};
	{
		$name:ident @ $address:literal
		$($fn:ident $setfn:ident [$param:tt] $ty:ty)*
	} => {
		pub struct $name(u32);

		impl $name {
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
		#[derive(Clone, Copy, Debug)]
		pub enum $name {
			$($variant = $val,)*
		}

		impl PanicFrom<u32> for $name {
			fn panic_from(value: u32) {
				match value {
					$($val => Self::$variant,)*
					_ => unreachable!(),
				}
			}
		}
	};
	{
		try $name:ident
		$($variant:ident $val:literal)*
	} => {
		#[derive(Clone, Copy, Debug)]
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
	};
}

macro_rules! log {
	($($arg:tt)*) => {
		let _ = rt::io::stderr().map(|o| writeln!(o, $($arg)*));
	};
}

mod control;
mod gmbus;
mod plane;
mod vga;

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

				// This is the only errata I found wrt. GMBUS. (see vol15)
				// It doesn't seem to do anything though.
				/*
				unsafe {
					let v = control.load(0xc2020);
					control.store(0xc2020, v | (1 << 12));
				}
				*/

				use gmbus::PinPair;
				for pp in [PinPair::DacDdc, PinPair::DdiB, PinPair::DdiC, PinPair::DdiD] {
					rt::thread::sleep(core::time::Duration::from_secs(5));
					log!("{:?}", pp);
					unsafe {
						// Reset GMBUS controller
						let mut cmd = gmbus::Gmbus1::from_raw(0);
						cmd.set_software_clear_interrupt(true);
						control.store(gmbus::Gmbus1::REG, cmd.as_raw());
						cmd.set_software_clear_interrupt(false);
						control.store(gmbus::Gmbus1::REG, cmd.as_raw());

						control.store(gmbus::Gmbus5::REG, 0); // just in case

						// Select device
						let mut select = gmbus::Gmbus0::from_raw(4);
						select.set_rate(gmbus::Rate::Hz100K);
						select.set_pin_pair(pp);
						log!("will wr {:#04x}", select.as_raw());
						control.store(gmbus::Gmbus0::REG, select.as_raw());
						log!(
							"{:#04x} -> {:#04x}",
							gmbus::Gmbus0::REG,
							control.load(gmbus::Gmbus0::REG)
						);
					}
					log!("gmbus0");

					let mut edid = [0; 128];
					unsafe {
						/*
						log!("edid wr");
						if let Err(e) = gmbus::write(&mut control, 0x50, &[0]) {
							log!("error: {:?}", e);
							continue;
						}
						*/
						log!("edid rd");
						if let Err(e) = gmbus::read(&mut control, 0x50, &mut edid) {
							log!("error: {:?}", e);
							continue;
						}
						log!("edid done");
					}
					for c in edid.as_chunks::<32>().0 {
						log!("{:02x?}", c);
					}
					break;
				}
			}
			_ => unreachable!(),
		}
	};

	log!("stop");
	rt::thread::sleep(core::time::Duration::MAX);
	0
}
