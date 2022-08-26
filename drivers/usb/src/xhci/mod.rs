//! # xHCI driver
//!
//! [1]: https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/extensible-host-controler-interface-usb-xhci.pdf

mod device;
mod errata;
mod event;
mod ring;

use errata::Errata;

use crate::{dma::Dma, requests};
use alloc::{collections::BTreeMap, vec::Vec};
use core::{mem, num::NonZeroU8, time::Duration};
use xhci::{
	registers::operational::DeviceContextBaseAddressArrayPointerRegister, ring::trb::command,
	Registers,
};

pub struct Xhci {
	event_ring: event::Table,
	command_ring: ring::Ring<command::Allowed>,
	registers: Registers<driver_utils::accessor::Identity>,
	dcbaap: DeviceContextBaseAddressArray,
	devices: BTreeMap<NonZeroU8, device::Device>,
	pending_commands: BTreeMap<ring::EntryId, PendingCommand>,
	transfers: BTreeMap<ring::EntryId, Dma<[u8]>>,
	wait_device_reset: Vec<device::WaitReset>,
	poll: rt::Object,
}

impl Xhci {
	pub fn new(dev: &rt::Object) -> Result<Self, &'static str> {
		let errata = Errata::get(0x1b36, 0x000d);

		let poll = dev.open(b"poll").unwrap();
		let (mmio_ptr, _) = dev
			.open(b"bar0")
			.unwrap()
			.map_object(None, rt::RWX::RW, 0, usize::MAX)
			.unwrap();

		let mut regs = unsafe {
			xhci::Registers::new(mmio_ptr.as_ptr() as _, driver_utils::accessor::Identity)
		};

		// 4.22.1 Pre-OS to OS Handoff Synchronization
		{
			use xhci::extended_capabilities::{ExtendedCapability, List};
			let ext = unsafe {
				List::new(
					mmio_ptr.as_ptr() as _,
					regs.capability.hccparams1.read_volatile(),
					driver_utils::accessor::Identity,
				)
			};
			for e in ext.into_iter().flat_map(|mut l| l.into_iter()) {
				match e {
					Ok(ExtendedCapability::UsbLegacySupport(mut c)) => {
						// Wait for BIOS to yield control
						c.usblegsup.update_volatile(|c| {
							c.set_hc_os_owned_semaphore();
						});
						while c.usblegsup.read_volatile().hc_bios_owned_semaphore() {
							rt::thread::sleep(core::time::Duration::from_millis(1));
						}
					}
					_ => {}
				}
			}
		}

		// 4.2 Host Controller Initialization
		let dcbaap = DeviceContextBaseAddressArray::new().unwrap_or_else(|_| todo!());
		let command_ring = ring::Ring::new().unwrap_or_else(|_| todo!());
		let mut event_ring = event::Table::new().unwrap_or_else(|_| todo!());
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
			event_ring.install(regs.interrupter_register_set.interrupter_mut(0));

			regs.operational.usbcmd.update_volatile(|c| {
				c.set_interrupter_enable();
			});

			regs.interrupter_register_set
				.interrupter_mut(0)
				.iman
				.update_volatile(|c| {
					c.set_interrupt_enable();
				});

			// Write the USBCMD (5.4.1) to turn the host controller ON
			regs.operational.usbcmd.update_volatile(|c| {
				c.set_run_stop();
			});
		}

		// QEMU is buggy and doesn't generate PSCEs at reset unless we reset the ports, so do that.
		if errata.no_psce_on_reset() {
			for i in 0..regs.port_register_set.len() {
				regs.port_register_set.update_volatile_at(i, |c| {
					c.portsc.set_port_reset();
				});
			}
		}

		Ok(Self {
			event_ring,
			command_ring,
			registers: regs,
			dcbaap,
			devices: Default::default(),
			pending_commands: Default::default(),
			transfers: Default::default(),
			wait_device_reset: Default::default(),
			poll,
		})
	}

	fn enqueue_command(&mut self, cmd: command::Allowed) -> Result<ring::EntryId, ring::Full> {
		let e = self.command_ring.enqueue(cmd);
		self.registers.doorbell.update_volatile_at(0, |c| {
			c.set_doorbell_stream_id(0).set_doorbell_target(0);
		});
		Ok(e)
	}

	pub fn send_request(
		&mut self,
		slot: NonZeroU8,
		req: crate::requests::Request,
	) -> Result<ring::EntryId, ring::Full> {
		let mut req = req.into_raw();
		let id = self.devices.get_mut(&slot).unwrap().send_request(0, &req)?;
		self.ring(slot.get(), 0, 1);
		req.buffer.take().map(|b| self.transfers.insert(id, b));
		Ok(id)
	}

	pub fn transfer(
		&mut self,
		slot: NonZeroU8,
		endpoint: u8,
		data: Dma<[u8]>,
		notify: bool,
	) -> Result<ring::EntryId, ring::Full> {
		let id = self
			.devices
			.get_mut(&slot)
			.expect("no device at slot")
			.transfer(endpoint, notify.then(|| 0), &data)?;
		self.ring(slot.get(), 0, endpoint);
		self.transfers.insert(id, data);
		Ok(id)
	}

	pub fn configure_device(
		&mut self,
		slot: NonZeroU8,
		config: DeviceConfig,
	) -> Result<ring::EntryId, ring::Full> {
		let (cmd, buf) = self
			.devices
			.get_mut(&slot)
			.expect("no device")
			.configure(config);
		core::mem::forget(buf); // FIXME
		self.enqueue_command(cmd).inspect(|&id| {
			self.pending_commands
				.insert(id, PendingCommand::ConfigureDev);
		})
	}

	pub fn poll(&mut self) -> Option<Event> {
		for i in (0..self.wait_device_reset.len()).rev() {
			if let Some((cmd, e)) = self.wait_device_reset[i].poll(&mut self.registers) {
				self.wait_device_reset.swap_remove(i);
				let id = self.enqueue_command(cmd).unwrap_or_else(|_| todo!());
				self.pending_commands
					.insert(id, PendingCommand::AllocSlot(e));
				if self.wait_device_reset.is_empty() {
					// Free the memory as wait_device_reset should only be very uncommonly used
					// anyways.
					self.wait_device_reset = Vec::new();
				}
			}
		}
		loop {
			let evt = if let Some(evt) = self.event_ring.dequeue() {
				self.event_ring
					.inform(self.registers.interrupter_register_set.interrupter_mut(0));
				evt
			} else {
				return None;
			};
			return Some(match evt {
				event::Event::PortStatusChange { port } => {
					let e = device::init(port, self).unwrap();
					self.wait_device_reset.push(e);
					continue;
				}
				event::Event::CommandCompletion { id, slot, code } => {
					match self.pending_commands.remove(&id).unwrap() {
						PendingCommand::AllocSlot(mut e) => {
							let (id, e) = e.init(self, slot).unwrap_or_else(|_| todo!());
							self.pending_commands
								.insert(id, PendingCommand::SetAddress(e));
							continue;
						}
						PendingCommand::SetAddress(e) => {
							let d = e.finish();
							let d = self
								.devices
								.entry(d.slot())
								.and_modify(|_| panic!("slot already occupied"))
								.or_insert(d);
							Event::NewDevice { slot: d.slot() }
						}
						PendingCommand::ConfigureDev => Event::DeviceConfigured { id, slot, code },
					}
				}
				event::Event::Transfer {
					id,
					endpoint,
					slot,
					code,
				} => Event::Transfer {
					id,
					slot,
					endpoint,
					buffer: self.transfers.remove(&id),
					code,
				},
			});
		}
	}

	/// Get the next slot after the given slot.
	pub fn next_slot(&self, slot: Option<NonZeroU8>) -> Option<NonZeroU8> {
		slot.map_or(0, |n| n.get())
			.checked_add(1)
			.and_then(|n| self.devices.range(NonZeroU8::new(n).unwrap()..).next())
			.map(|(k, _)| *k)
	}

	fn ring(&mut self, slot: u8, stream: u16, endpoint: u8) {
		// SAFETY: 0 is a valid value for a doorbell and Register is repr(transparent) of u32.
		// Annoyingly, the xhci crate doesn't provide a Default impl or anything for it, so
		// TODO make a PR
		let mut v = unsafe { mem::transmute::<_, xhci::registers::doorbell::Register>(0u32) };
		v.set_doorbell_stream_id(stream)
			.set_doorbell_target(endpoint);
		self.registers.doorbell.write_volatile_at(slot.into(), v);
	}
}

struct DeviceContextBaseAddressArray {
	storage: Dma<[u64; 256]>,
}

impl DeviceContextBaseAddressArray {
	fn new() -> Result<Self, rt::Error> {
		Dma::new().map(|storage| Self { storage })
	}

	fn install(&self, reg: &mut DeviceContextBaseAddressArrayPointerRegister) {
		reg.set(self.storage.as_phys())
	}

	fn set(&mut self, slot: NonZeroU8, phys: u64) {
		unsafe { self.storage.as_mut()[usize::from(slot.get())] = phys }
	}
}

enum PendingCommand {
	AllocSlot(device::AllocSlot),
	SetAddress(device::SetAddress),
	ConfigureDev,
}

pub enum Event {
	NewDevice {
		slot: NonZeroU8,
	},
	Transfer {
		slot: NonZeroU8,
		endpoint: u8,
		buffer: Option<Dma<[u8]>>,
		id: ring::EntryId,
		code: Result<xhci::ring::trb::event::CompletionCode, u8>,
	},
	DeviceConfigured {
		slot: NonZeroU8,
		id: ring::EntryId,
		code: Result<xhci::ring::trb::event::CompletionCode, u8>,
	},
}

pub struct DeviceConfig {
	pub config: requests::Configuration,
	pub interface: requests::Interface,
	// TODO avoid allocation
	pub endpoints: Vec<requests::Endpoint>,
}
