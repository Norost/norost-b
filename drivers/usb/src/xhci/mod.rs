//! # xHCI driver
//!
//! [1]: https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/extensible-host-controler-interface-usb-xhci.pdf

mod command;
mod device;
mod errata;
mod event;
mod port;
mod ring;

use errata::Errata;

use crate::{dma::Dma, requests};
use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use command::Pending;
use core::{mem, num::NonZeroU8, time::Duration};
use xhci::ring::trb::command as cmd;

pub use device::TransferError;

type Registers = xhci::Registers<driver_utils::accessor::Identity>;

pub struct Xhci {
	event_ring: event::Table,
	command_ring: ring::Ring<cmd::Allowed>,
	registers: Registers,
	dcbaa: DeviceContextBaseAddressArray,
	devices: BTreeMap<NonZeroU8, device::Device>,
	pending: BTreeMap<ring::EntryId, Pending>,
	transfers: BTreeMap<ring::EntryId, Dma<[u8]>>,
	poll: rt::Object,
	transfers_config_packet_size: BTreeMap<ring::EntryId, (device::SetAddress, Dma<[u8]>)>,
	port_slot_map: [Option<NonZeroU8>; 255],
}

impl Xhci {
	pub fn new(dev: &rt::Object) -> Result<Self, &'static str> {
		trace!("get errata");
		let mut errata = unsafe {
			let (pci, s) = dev.map_object(None, rt::RWX::R, 0, 4096).unwrap();
			assert_eq!(s, 4096);
			let vendor = pci.cast::<u16>().as_ptr().add(0).read_volatile();
			let device = pci.cast::<u16>().as_ptr().add(1).read_volatile();
			let _ = rt::mem::dealloc(pci, 4096);
			Errata::get(vendor, device)
		};

		let poll = dev.open(b"poll").unwrap();
		trace!("map MMIO");
		let (mmio_ptr, _) = dev
			.open(b"bar0")
			.unwrap()
			.map_object(None, rt::RWX::RW, 0, usize::MAX)
			.unwrap();

		trace!("wrap MMIO {:p}", mmio_ptr);
		let mut regs = unsafe {
			xhci::Registers::new(mmio_ptr.as_ptr() as _, driver_utils::accessor::Identity)
		};

		assert!(
			!regs.capability.hccparams1.read_volatile().context_size(),
			"todo: 64 byte context"
		);

		// 4.22.1 Pre-OS to OS Handoff Synchronization
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
					trace!("wait for BIOS handover");
					// Wait for BIOS to yield control
					c.usblegsup.update_volatile(|c| {
						c.set_hc_os_owned_semaphore();
					});
					while c.usblegsup.read_volatile().hc_bios_owned_semaphore() {
						rt::thread::sleep(core::time::Duration::from_millis(1));
					}
				}
				Ok(ExtendedCapability::XhciSupportedProtocol(mut c)) => {
					// TODO use this to determine USB types
					rt::dbg!(&c);
				}
				Ok(ExtendedCapability::Debug(_)) => {}
				Ok(ExtendedCapability::HciExtendedPowerManagementCapability(_)) => {}
				Ok(ExtendedCapability::XhciMessageInterrupt(_)) => {}
				Ok(ExtendedCapability::XhciLocalMemory(_)) => {}
				Ok(ExtendedCapability::XhciExtendedMessageInterrupt(_)) => {}
				// 192 = Intel stuff
				Err(xhci::extended_capabilities::NotSupportedId(192)) => errata.set_intel_vendor(),
				Err(xhci::extended_capabilities::NotSupportedId(i)) => {
					trace!("unknown ext cap {}", i)
				}
			}
		}

		let oper = &mut regs.operational;

		// Wait for controller to be ready
		while oper.usbsts.read_volatile().controller_not_ready() {
			rt::thread::sleep(Duration::from_millis(1));
		}

		trace!("stop controller");
		oper.usbcmd.update_volatile(|c| {
			c.clear_run_stop();
		});
		assert!(oper.usbsts.read_volatile().hc_halted());

		// 4.2 Host Controller Initialization
		trace!("reset & initialize controller");
		let command_ring = ring::Ring::new().unwrap_or_else(|_| todo!());
		let mut event_ring = event::Table::new().unwrap_or_else(|_| todo!());

		// After Chip Hardware Reset ...
		oper.usbcmd.update_volatile(|c| {
			c.set_host_controller_reset();
		});

		if errata.hang_after_reset() {
			trace!("wait 1ms to avoid hang after reset");
			rt::thread::sleep(Duration::from_millis(1));
		}

		// FIXME We are probably doing something wrong, this delay shouldn't be necessary
		// Either that or we found an errata (hooray!)
		rt::thread::sleep(Duration::from_millis(500));

		while oper.usbcmd.read_volatile().host_controller_reset() {
			rt::thread::sleep(Duration::from_millis(1));
		}
		while oper.usbsts.read_volatile().controller_not_ready() {
			rt::thread::sleep(Duration::from_millis(1));
		}

		// Program the Max Device Slots Enabled (MaxSlotsEn) field
		oper.config.update_volatile(|c| {
			let n = regs
				.capability
				.hcsparams1
				.read_volatile()
				.number_of_device_slots();
			trace!("{} device slots", n);
			c.set_max_device_slots_enabled(n);
		});

		// Program the Device Context Base Address Array Pointer (DCBAAP)
		let dcbaa = DeviceContextBaseAddressArray::new(&mut regs).unwrap_or_else(|_| todo!());

		assert!(!regs.operational.usbcmd.read_volatile().run_stop());

		// Define the Command Ring Dequeue Pointer
		regs.operational.crcr.update_volatile(|c| {
			c.set_ring_cycle_state();
			//c.clear_ring_cycle_state();
			c.set_command_ring_pointer(command_ring.as_phys());
		});

		// Initialize interrupts by:

		// Initialize each active interrupter by:

		// Defining the Event Ring:
		event_ring.install(regs.interrupter_register_set.interrupter_mut(0));

		regs.operational.usbcmd.update_volatile(|c| {
			c.set_interrupter_enable();
		});

		let mut intr = regs.interrupter_register_set.interrupter_mut(0);
		intr.imod.update_volatile(|c| {
			c.set_interrupt_moderation_interval(4000); // 1ms
		});
		intr.iman.update_volatile(|c| {
			c.set_interrupt_enable();
		});

		trace!("enable controller");
		// Write the USBCMD (5.4.1) to turn the host controller ON
		regs.operational.usbcmd.update_volatile(|c| {
			c.set_run_stop();
		});

		// QEMU is buggy and doesn't generate PSCEs at reset unless we reset the ports, so do that.
		if errata.no_psce_on_reset() {
			trace!("apply PSCE errate fix");
			for i in 0..regs.capability.hcsparams1.read_volatile().number_of_ports() {
				regs.port_register_set
					.port_register_set_mut(i.into())
					.portsc
					.update_volatile(|c| {
						c.unset_port_enabled_disabled();
						c.set_port_reset();
					});
			}
		}

		Ok(Self {
			event_ring,
			command_ring,
			registers: regs,
			dcbaa,
			devices: Default::default(),
			pending: Default::default(),
			transfers: Default::default(),
			poll,
			transfers_config_packet_size: Default::default(),
			port_slot_map: [None; 255],
		})
	}

	pub fn send_request(
		&mut self,
		slot: NonZeroU8,
		req: crate::requests::Request,
	) -> Result<ring::EntryId, ()> {
		trace!("send request, slot {}", slot);
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
	) -> Result<ring::EntryId, TransferError> {
		trace!(
			"transfer, slot {} ep {}, data len {}",
			slot,
			endpoint,
			data.len()
		);
		let id = self
			.devices
			.get_mut(&slot)
			.expect("no device at slot")
			.transfer(endpoint, notify.then(|| 0), &data)?;
		self.ring(slot.get(), 0, endpoint);
		self.transfers.insert(id, data);
		Ok(id)
	}

	pub fn configure_device(&mut self, slot: NonZeroU8, config: DeviceConfig<'_>) -> ring::EntryId {
		trace!("configure device, slot {}", slot);
		let (cmd, buf) = self
			.devices
			.get_mut(&slot)
			.expect("no device")
			.configure(config);
		core::mem::forget(buf); // FIXME
		self.enqueue_command(cmd, Pending::ConfigureDev)
	}

	pub fn poll(&mut self) -> Option<Event> {
		trace!("poll");
		use xhci::ring::trb::event::{Allowed, CompletionCode};
		loop {
			let evt = if let Some(evt) = self.event_ring.dequeue() {
				self.event_ring
					.inform(self.registers.interrupter_register_set.interrupter_mut(0));
				evt
			} else {
				trace!("no events");
				return None;
			};
			return Some(match evt {
				Allowed::Doorbell(_) => todo!(),
				Allowed::MfindexWrap(_) => todo!(),
				Allowed::TransferEvent(c) => {
					let slot = NonZeroU8::new(c.slot_id()).expect("transfer event on slot 0");
					let endpoint = c.endpoint_id();
					let id = c.trb_pointer();
					let code = c.completion_code();
					trace!(
						"transfer event slot {} ep {} id {:x}, {:?}",
						slot,
						endpoint,
						id,
						code
					);
					if let Some((mut e, buf)) = self.transfers_config_packet_size.remove(&id) {
						let size = unsafe { buf.as_ref()[7] };
						trace!("reconfigure packet size to {}", size);
						let cmd = e.adjust_packet_size(size);
						self.enqueue_command(cmd, Pending::SetAddress(e));
						continue;
					}
					Event::Transfer {
						id,
						slot,
						endpoint,
						buffer: self.transfers.remove(&id),
						code,
					}
				}
				Allowed::HostController(_) => todo!(),
				Allowed::PortStatusChange(c) => {
					self.handle_port_status_change(c);
					continue;
				}
				Allowed::BandwidthRequest(_) => todo!(),
				Allowed::CommandCompletion(c) => {
					if let Some(e) = self.handle_pending(c) {
						e
					} else {
						continue;
					}
				}
				Allowed::DeviceNotification(_) => todo!(),
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

	pub fn notifier(&self) -> rt::RefObject<'_> {
		(&self.poll).into()
	}

	fn ring(&mut self, slot: u8, stream: u16, endpoint: u8) {
		trace!(
			"ring doorbell, slot {} stream {} ep {}",
			slot,
			stream,
			endpoint
		);
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
	scratchpad_array: Dma<[u64]>,
	scratchpad_pages: Box<[Dma<[u8; 4096]>]>,
}

impl DeviceContextBaseAddressArray {
	fn new(regs: &mut Registers) -> Result<Self, rt::Error> {
		trace!("init DCBAA");
		let sp_count = regs
			.capability
			.hcsparams2
			.read_volatile()
			.max_scratchpad_buffers();
		let sp_count = usize::try_from(sp_count).unwrap();
		trace!("{} scratch pages", sp_count);
		let mut storage = Dma::<[u64; 256]>::new()?;
		let mut scratchpad_array = Dma::new_slice(sp_count)?;
		let scratchpad_pages = (0..sp_count).map(|_| Dma::new()).try_collect::<Box<_>>()?;
		for (e, p) in unsafe { scratchpad_array.as_mut() }
			.iter_mut()
			.zip(&*scratchpad_pages)
		{
			*e = p.as_phys();
		}
		trace!("done alloc");
		unsafe { storage.as_mut()[0] = scratchpad_array.as_phys() }
		regs.operational.dcbaap.update_volatile(|c| {
			c.set(storage.as_phys());
		});
		Ok(Self {
			storage,
			scratchpad_array,
			scratchpad_pages,
		})
	}

	fn set(&mut self, slot: NonZeroU8, phys: u64) {
		unsafe { self.storage.as_mut()[usize::from(slot.get())] = phys }
	}
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

pub struct DeviceConfig<'a> {
	pub config: &'a requests::Configuration,
	pub interface: &'a requests::Interface,
	pub endpoints: &'a [requests::Endpoint],
}
