//! # xHCI driver
//!
//! [1]: https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/extensible-host-controler-interface-usb-xhci.pdf

pub mod device;
mod errata;
mod event;
mod ring;

use errata::Errata;

use alloc::vec::Vec;
use core::{
	mem,
	num::{NonZeroU8, NonZeroUsize},
	ptr::NonNull,
	time::Duration,
};
use driver_utils::dma;
use xhci::{
	registers::operational::DeviceContextBaseAddressArrayPointerRegister, ring::trb::command,
	Registers,
};

pub use event::Event;

const PCI_CLASS: u8 = 0x0c;
const PCI_SUBCLASS: u8 = 0x03;
const PCI_INTERFACE: u8 = 0x30;

fn is_interface() {}

pub struct Xhci {
	event_ring: event::Table,
	command_ring: ring::Ring<command::Allowed>,
	registers: Registers<driver_utils::accessor::Identity>,
	dcbaap: DeviceContextBaseAddressArray,
}

impl Xhci {
	pub fn new(dev: rt::Object) -> Result<Self, &'static str> {
		let errata = Errata::get(0x1b36, 0x000d);

		let poll = dev.open(b"poll").unwrap();
		let pci_config = dev.map_object(None, rt::RWX::R, 0, usize::MAX).unwrap().0;
		let (mmio_ptr, mmio_len) = dev
			.open(b"bar0")
			.unwrap()
			.map_object(None, rt::RWX::RW, 0, usize::MAX)
			.unwrap();

		let mut regs = unsafe {
			xhci::Registers::new(mmio_ptr.as_ptr() as _, driver_utils::accessor::Identity)
		};

		// 4.2 Host Controller Initialization
		let dcbaap = DeviceContextBaseAddressArray::new().unwrap_or_else(|_| todo!());
		let command_ring = ring::Ring::new().unwrap_or_else(|_| todo!());
		let event_ring = event::Table::new().unwrap_or_else(|_| todo!());
		{
			// After Chip Hardware Reset ...
			regs.operational.usbcmd.update_volatile(|c| {
				c.set_host_controller_reset();
			});

			// ... wait until the Controller Not Ready (CNR) flag is 0
			while regs
				.operational
				.usbsts
				.read_volatile()
				.controller_not_ready()
			{
				rt::thread::sleep(Duration::from_millis(1));
			}

			// Program the Max Device Slots Enabled (MaxSlotsEn) field
			regs.operational.config.update_volatile(|c| {
				c.set_max_device_slots_enabled(1);
			});

			// Program the Device Context Base Address Array Pointer (DCBAAP)
			regs.operational
				.dcbaap
				.update_volatile(|c| dcbaap.install(c));

			// Define the Command Ring Dequeue Pointer
			regs.operational.crcr.update_volatile(|c| {
				c.set_command_ring_pointer(command_ring.as_phys());
			});

			// Initialize interrupts by:

			// ... TODO actual interrupts (which are optional anyways)

			// Initialize each active interrupter by:

			// Defining the Event Ring:
			regs.interrupt_register_set
				.update_volatile_at(0, |c| event_ring.install(c));

			regs.operational.usbcmd.update_volatile(|c| {
				c.set_interrupter_enable();
			});

			regs.interrupt_register_set.update_volatile_at(0, |c| {
				c.iman.set_interrupt_enable();
			});

			// Write the USBCMD (5.4.1) to turn the host controller ON
			regs.operational.usbcmd.update_volatile(|c| {
				c.set_run_stop();
			});
		}

		// QEMU is buggy and doesn't generate PSCEs at reset unless we reset the ports, so do that.
		rt::dbg!();
		if errata.no_psce_on_reset() {
			for i in 0..regs.port_register_set.len() {
				regs.port_register_set.update_volatile_at(i, |c| {
					c.portsc.set_port_reset();
				});
			}
		}
		rt::dbg!();

		Ok(Self {
			event_ring,
			command_ring,
			registers: regs,
			dcbaap,
		})
	}

	pub fn enqueue_command(&mut self, cmd: command::Allowed) -> Result<ring::EntryId, ring::Full> {
		let e = self.command_ring.enqueue(cmd)?;
		self.registers.doorbell.update_volatile_at(0, |c| {
			c.set_doorbell_stream_id(0).set_doorbell_target(0);
		});
		Ok(e)
	}

	pub fn dequeue_event(&mut self) -> Option<Event> {
		self.event_ring.dequeue()
	}

	pub fn init_device(&mut self, port: NonZeroU8) -> Result<device::WaitReset, &'static str> {
		device::init(port, self)
	}
}

struct DeviceContextBaseAddressArray {
	ptr: NonNull<u64>,
	phys: u64,
}

impl DeviceContextBaseAddressArray {
	fn new() -> Result<Self, rt::Error> {
		let (ptr, phys, _) = dma::alloc_dma(2048.try_into().unwrap())?;
		Ok(Self {
			ptr: ptr.cast(),
			phys,
		})
	}

	fn install(&self, reg: &mut DeviceContextBaseAddressArrayPointerRegister) {
		reg.set(self.phys)
	}

	fn set(&mut self, slot: NonZeroU8, phys: u64) {
		unsafe { self.ptr.as_ptr().add(slot.get().into()).write(phys) }
	}
}
