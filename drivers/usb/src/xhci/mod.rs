//! # xHCI driver
//!
//! [1]: https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/extensible-host-controler-interface-usb-xhci.pdf

mod device;
mod errata;
mod event;
mod ring;

use errata::Errata;

use crate::{dma::Dma, requests};
use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};
use core::{mem, num::NonZeroU8, time::Duration};
use xhci::ring::trb::command;

type Registers = xhci::Registers<driver_utils::accessor::Identity>;

pub struct Xhci {
	event_ring: event::Table,
	command_ring: ring::Ring<command::Allowed>,
	registers: Registers,
	dcbaap: DeviceContextBaseAddressArray,
	devices: BTreeMap<NonZeroU8, device::Device>,
	setup_devices: BTreeMap<NonZeroU8, PendingCommand>,
	pending_commands: BTreeMap<ring::EntryId, PendingCommand>,
	transfers: BTreeMap<ring::EntryId, Dma<[u8]>>,
	poll: rt::Object,
	transfers_config_packet_size: BTreeMap<ring::EntryId, (device::SetAddress, Dma<[u8]>)>,
}

impl Xhci {
	pub fn new(dev: &rt::Object) -> Result<Self, &'static str> {
		trace!("get errata");
		let errata = unsafe {
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
		trace!("quack");

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
				_ => {}
			}
		}

		// 4.2 Host Controller Initialization
		trace!("reset & initialize controller");
		let command_ring = ring::Ring::new().unwrap_or_else(|_| todo!());
		let mut event_ring = event::Table::new().unwrap_or_else(|_| todo!());

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
			rt::dbg!("zzz");
			rt::thread::sleep(Duration::from_millis(1));
		}

		// Program the Max Device Slots Enabled (MaxSlotsEn) field
		regs.operational.config.update_volatile(|c| {
			let n = regs
				.capability
				.hcsparams1
				.read_volatile()
				.number_of_device_slots();
			trace!("{} device slots", n);
			c.set_max_device_slots_enabled(n);
		});

		// Program the Device Context Base Address Array Pointer (DCBAAP)
		let dcbaap = DeviceContextBaseAddressArray::new(&mut regs).unwrap_or_else(|_| todo!());

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

		regs.interrupter_register_set
			.interrupter_mut(0)
			.imod
			.update_volatile(|c| {
				c.set_interrupt_moderation_interval(4000); // 1ms
			});
		regs.interrupter_register_set
			.interrupter_mut(0)
			.iman
			.update_volatile(|c| {
				c.set_interrupt_enable();
			});

		trace!("enable controller");
		// Write the USBCMD (5.4.1) to turn the host controller ON
		regs.operational.usbcmd.update_volatile(|c| {
			c.set_run_stop();
		});

		rt::thread::sleep(core::time::Duration::from_millis(100));

		// QEMU is buggy and doesn't generate PSCEs at reset unless we reset the ports, so do that.
		if errata.no_psce_on_reset() {
			trace!("apply PSCE errate fix");
			for i in 0..regs.capability.hcsparams1.read_volatile().number_of_ports() {
				regs.port_register_set.update_volatile_at(i.into(), |c| {
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
			setup_devices: Default::default(),
			pending_commands: Default::default(),
			transfers: Default::default(),
			poll,
			transfers_config_packet_size: Default::default(),
		})
	}

	fn enqueue_command(&mut self, cmd: command::Allowed) -> Result<ring::EntryId, ring::Full> {
		trace!("enqueue command {:?}", &cmd);
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
	) -> Result<ring::EntryId, ring::Full> {
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

	pub fn configure_device(
		&mut self,
		slot: NonZeroU8,
		config: DeviceConfig,
	) -> Result<ring::EntryId, ring::Full> {
		trace!("configure device, slot {}", slot);
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
		trace!("poll");
		use xhci::ring::trb::event::CompletionCode;
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
				event::Event::PortStatusChange { port } => {
					trace!("event: port {} status change", port);
					self.handle_port_status_change(port);
					continue;
				}
				event::Event::CommandCompletion { id, slot, code } => {
					trace!(
						"event: command completed, slot {:?} id {:x}, {:?}",
						slot,
						id,
						code
					);
					if code != Ok(CompletionCode::Success) {
						continue;
					}
					assert_eq!(code, Ok(CompletionCode::Success));
					let slot = slot.unwrap();
					match self.pending_commands.remove(&id).unwrap() {
						PendingCommand::AllocSlot(mut e) => {
							trace!("allocated slot, set address");
							let (id, e) = e.init(self, slot).unwrap_or_else(|_| todo!());
							self.pending_commands
								.insert(id, PendingCommand::SetAddress(e));
							continue;
						}
						PendingCommand::SetAddress(mut e) => {
							if e.should_adjust_packet_size() {
								trace!("adjust packet size: get descriptor");
								let req = crate::requests::Request::GetDescriptor {
									buffer: Dma::new_slice(8).unwrap(),
									ty: requests::GetDescriptor::Device,
								}
								.into_raw();
								let id = e.dev.send_request(0, &req).unwrap_or_else(|_| todo!());
								self.ring(e.dev.slot().get(), 0, 1);
								let buf = req.buffer.unwrap();
								self.transfers_config_packet_size.insert(id, (e, buf));
								continue;
							}
							trace!("finish set address, configure device");
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
				} => {
					trace!(
						"event: transfer, port {} ep {} id {:x}, {:?}",
						slot,
						endpoint,
						id,
						code
					);
					if let Some((mut e, buf)) = self.transfers_config_packet_size.remove(&id) {
						rt::dbg!(unsafe { buf.as_ref() });
						let size = unsafe { buf.as_ref()[7] };
						trace!("reconfigure packet size to {}", size);
						let cmd = e.adjust_packet_size(size);
						let id = self.enqueue_command(cmd).unwrap_or_else(|_| todo!());
						self.pending_commands
							.insert(id, PendingCommand::SetAddress(e));
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

	/// Perform a test to see if the device works properly
	#[allow(dead_code)]
	pub fn test(&mut self) {
		trace!("test: send noop");
		let i = self
			.enqueue_command(command::Allowed::Noop(command::Noop::new()))
			.unwrap_or_else(|_| panic!("failed to enqueue command"));
		trace!("test: wait for noop, id {}", i);
		self.poll.read(&mut []).expect("wait for interrupt failed");
		match self.event_ring.dequeue().expect("no events enqueued") {
			event::Event::CommandCompletion { id, slot, code } => {
				trace!(
					"test: got command completion, id {} slot {:?} code {:?}",
					id,
					slot,
					code
				);
				assert!(i == id);
				assert!(code == Ok(xhci::ring::trb::event::CompletionCode::Success));
				assert!(slot.is_none());
			}
			_ => panic!("unexpected event"),
		}
		trace!("test: success");
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
