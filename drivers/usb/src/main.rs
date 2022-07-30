//! # xHCI driver
//!
//! [1]: https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/extensible-host-controler-interface-usb-xhci.pdf

#![no_std]
#![feature(start)]

extern crate alloc;

use alloc::vec::Vec;
use core::{mem, slice, time::Duration};
use driver_utils::os::stream_table::{Request, Response, StreamTable};
use rt::{io::Pow2Size, Handle};
use rt_default as _;

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main()
}

fn main() -> ! {
	let file_root = rt::io::file_root().expect("no file root");
	/*
	let table_name = rt::args::Args::new()
		.skip(1)
		.next()
		.expect("expected table name");
	*/

	let (dev_handle, errata) = {
		let s = b" 1b36:000d";
		let errata = Errata::PCI_1B36_000D;
		let mut it = file_root.open(b"pci/info").unwrap();
		let mut buf = [0; 64];
		loop {
			let l = it.read(&mut buf).unwrap();
			assert!(l != 0, "device not found");
			let dev = &buf[..l];
			if dev.ends_with(s) {
				let mut path = Vec::from(*b"pci/");
				path.extend(&dev[..7]);
				break (file_root.open(&path).unwrap(), errata);
			}
		}
	};

	let poll = dev_handle.open(b"poll").unwrap();
	let pci_config = dev_handle
		.map_object(None, rt::RWX::R, 0, usize::MAX)
		.unwrap()
		.0;
	let (mmio_ptr, mmio_len) = dev_handle
		.open(b"bar0")
		.unwrap()
		.map_object(None, rt::RWX::RW, 0, usize::MAX)
		.unwrap();

	let mut regs =
		unsafe { xhci::Registers::new(mmio_ptr.as_ptr() as _, driver_utils::accessor::Identity) };

	let (dma_ptr, dma_phys, dma_len) =
		driver_utils::dma::alloc_dma((1 << 16).try_into().unwrap()).unwrap();

	let mut dma_offt = 0;
	let mut dma_alloc = |size: usize| {
		// Bah
		let size = (size + 63) & !63;
		assert!(dma_offt + size < dma_len.get(), "out of DMA pages");
		let (v, p) = (
			unsafe { dma_ptr.as_ptr().add(dma_offt) },
			dma_phys + dma_offt as u64,
		);
		dma_offt += size;
		(v, p)
	};

	// Init message is annoying.
	rt::thread::sleep(Duration::from_millis(1));

	// 4.2 Host Controller Initialization
	let (dcbaap_ptr, dcbaap_phys);
	let (crcr_ptr, crcr_phys);
	let (evt_ptr, evt_phys);
	let (evt_tbl_ptr, evt_tbl_phys);
	{
		// After Chip Hardware Reset ...
		regs.operational.usbcmd.update(|c| {
			c.set_host_controller_reset();
		});

		// ... wait until the Controller Not Ready (CNR) flag is 0
		while regs.operational.usbsts.read().controller_not_ready() {
			rt::thread::sleep(Duration::from_millis(1));
		}

		// Program the Max Device Slots Enabled (MaxSlotsEn) field
		regs.operational.config.update(|c| {
			c.set_max_device_slots_enabled(1);
		});

		// Program the Device Context Base Address Array Pointer (DCBAAP)
		(dcbaap_ptr, dcbaap_phys) = dma_alloc(mem::size_of::<xhci::context::Device32Byte>() * 1);
		regs.operational.dcbaap.update(|c| {
			c.set(dcbaap_phys);
		});

		// Define the Command Ring Dequeue Pointer
		(crcr_ptr, crcr_phys) = dma_alloc(16 * 16);
		regs.operational.crcr.update(|c| {
			c.set_command_ring_pointer(crcr_phys);
		});

		// Initialize interrupts by:

		// ... TODO actual interrupts (which are optional anyways)

		// Initialize each active interrupter by:

		// Defining the Event Ring:

		// Allocate and initialize the Event Ring Segment(s).
		(evt_ptr, evt_phys) = dma_alloc(16 * 16);

		// Allocate the Event Ring Segment Table.
		(evt_tbl_ptr, evt_tbl_phys) = dma_alloc(64 * 1);

		// Initialize ERST table entries to point to and to define the size (in TRBs) of the respective Event Ring Segment.
		#[repr(C)]
		struct EventRingSegmentTableEntry {
			ring_segment_base_address: u64,
			ring_segment_size: u16,
			_reserved: [u16; 3],
		}
		unsafe {
			assert_eq!(evt_phys & 0x1f, 0, "64 byte alignment");
			evt_tbl_ptr
				.cast::<EventRingSegmentTableEntry>()
				.write(EventRingSegmentTableEntry {
					ring_segment_base_address: evt_phys,
					ring_segment_size: 16,
					_reserved: [0; 3],
				});
		}

		regs.interrupt_register_set.update_at(0, |c| {
			// Program the Interrupter Event Ring Segment Table Size
			c.erstsz.set(1);
			// Program the Interrupter Event Ring Dequeue Pointer
			c.erdp.set_event_ring_dequeue_pointer(evt_phys);
			// Program the Interrupter Event Ring Segment Table Base Address
			c.erstba.set(evt_tbl_phys);
		});

		regs.operational.usbcmd.update(|c| {
			c.set_interrupter_enable();
		});

		regs.interrupt_register_set.update_at(0, |c| {
			c.iman.set_interrupt_enable();
		});

		// Write the USBCMD (5.4.1) to turn the host controller ON
		regs.operational.usbcmd.update(|c| {
			c.set_run_stop();
		});
	}

	// QEMU is buggy and doesn't generate PSCEs at reset unless we reset the ports, so do that.
	if errata.no_psce_on_reset() {
		for i in 0..regs.port_register_set.len() {
			regs.port_register_set.update_at(i, |c| {
				c.portsc.set_port_reset();
			});
		}
	}

	rt::thread::sleep(Duration::from_millis(100)); // idk, settle time, no interrupts, am lazy
	let mut cmd_enqueue_i = 0;
	let mut evt_dequeue_i = 0;

	rt::dbg!(regs.port_register_set.read_at(4));

	let mut ports = Vec::new();

	// 4.3 USB Device Initialization
	{
		use xhci::ring::trb::event::*;

		// Upon receipt of a Port Status Change Event system software evaluates the Port ID field
		loop {
			core::sync::atomic::fence(core::sync::atomic::Ordering::Acquire);
			let e = unsafe {
				evt_ptr
					.cast::<[u32; 4]>()
					.add(evt_dequeue_i)
					.read_volatile()
			};
			if e[3] & 1 == 0 {
				break;
			}
			evt_dequeue_i += 1;
			match Allowed::try_from(e).unwrap() {
				Allowed::PortStatusChange(p) => ports.push(p.port_id()),
				e => panic!("unexpected event {:?}", e),
			}
		}

		for &i in ports.iter() {
			let i = (i - 1).into();
			if regs
				.port_register_set
				.read_at(i)
				.portsc
				.current_connect_status()
			{
				// Reset if USB2
				// TODO check if USB2
				regs.port_register_set.update_at(i, |c| {
					c.portsc.set_port_reset();
				});
				while !regs.port_register_set.read_at(i).portsc.port_reset_change() {
					rt::thread::sleep(Duration::from_millis(1));
				}

				// system software shall obtain a Device Slot
				unsafe {
					crcr_ptr.cast::<[u32; 4]>().add(cmd_enqueue_i).write(
						xhci::ring::trb::command::EnableSlot::new()
							.set_slot_type(0)
							.set_cycle_bit()
							.into_raw(),
					);
					cmd_enqueue_i += 1;
					regs.doorbell.update_at(0, |c| {
						c.set_doorbell_stream_id(0).set_doorbell_target(0);
					});
				}
				loop {
					let e = unsafe {
						evt_ptr
							.cast::<[u32; 4]>()
							.add(evt_dequeue_i)
							.read_volatile()
					};
					if e[3] & 1 != 0 {
						break;
					}
					rt::thread::sleep(Duration::from_millis(1));
				}
				let e = unsafe {
					evt_ptr
						.cast::<[u32; 4]>()
						.add(evt_dequeue_i)
						.read_volatile()
				};
				let slot = match Allowed::try_from(e).unwrap() {
					Allowed::CommandCompletion(c) => c.slot_id(),
					e => panic!("unexpected event {:?}", e),
				};
				evt_dequeue_i += 1;

				// system software shall initialize the data structures associated with the slot
				let (input_ptr, input_phys);
				let (tr_ptr, tr_phys);
				let (output_dev_ptr, output_dev_phys);
				{
					use xhci::context::{InputControlHandler, InputHandler, SlotHandler};

					// Allocate an Input Context
					(input_ptr, input_phys) =
						dma_alloc(mem::size_of::<xhci::context::Input32Byte>());
					let input = unsafe { &mut *input_ptr.cast::<xhci::context::Input32Byte>() };

					// Set A0, A1
					input.control_mut().set_add_context_flag(0);
					input.control_mut().set_add_context_flag(1);

					// Initialize the Input Slot Context
					// FIXME how? what's the topology?
					input.device_mut().slot_mut().set_root_hub_port_number(slot);
					//input.device_mut().slot_mut().set_route_string(todo!());
					input.device_mut().slot_mut().set_context_entries(1);

					// Allocate and initialize the Transfer Ring for the Default Control Endpoint
					(tr_ptr, tr_phys) = dma_alloc(16 * 16);

					// Initialize the Input default control Endpoint 0 Context
					let ep = input.device_mut().endpoint_mut(1);
					ep.set_endpoint_type(xhci::context::EndpointType::Control);
					ep.set_max_packet_size(calc_packet_size(
						regs.port_register_set.read_at(i).portsc.port_speed(),
					));
					ep.set_max_burst_size(0);
					ep.set_tr_dequeue_pointer(tr_phys);
					ep.set_dequeue_cycle_state();
					ep.set_interval(0);
					ep.set_max_primary_streams(0);
					ep.set_mult(0);
					ep.set_error_count(0);

					// Allocate the Output Device Context data structure and set to '0'
					(output_dev_ptr, output_dev_phys) =
						dma_alloc(mem::size_of::<xhci::context::Device32Byte>());

					// Load the appropriate (Device Slot ID) entry in the Device Context Base Address Array
					unsafe {
						dcbaap_ptr
							.cast::<u64>()
							.add(slot.into())
							.write_volatile(output_dev_phys)
					};

					// Issue an Address Device Command for the Device Slot
					unsafe {
						crcr_ptr.cast::<[u32; 4]>().add(cmd_enqueue_i).write(
							xhci::ring::trb::command::AddressDevice::new()
								.set_slot_id(slot)
								.set_input_context_pointer(input_phys)
								.set_cycle_bit()
								.into_raw(),
						);
						cmd_enqueue_i += 1;
						regs.doorbell.update_at(0, |c| {
							c.set_doorbell_stream_id(0).set_doorbell_target(0);
						});
					}
					loop {
						let e = unsafe {
							evt_ptr
								.cast::<[u32; 4]>()
								.add(evt_dequeue_i)
								.read_volatile()
						};
						if e[3] & 1 != 0 {
							break;
						}
						rt::thread::sleep(Duration::from_millis(1));
					}
					let e = unsafe {
						evt_ptr
							.cast::<[u32; 4]>()
							.add(evt_dequeue_i)
							.read_volatile()
					};
					match Allowed::try_from(e).unwrap() {
						Allowed::CommandCompletion(c) => rt::dbg!(c),
						e => panic!("unexpected event {:?}", e),
					};
					evt_dequeue_i += 1;
				}

				// software will issue USB GET_DESCRIPTOR requests
				let mut tr_enqueue_i = 0;
				let (data_ptr, data_phys) = dma_alloc(64);
				{
					unsafe { rt::dbg!(data_ptr.cast::<u64>().read()) };
					// https://wiki.osdev.org/USB#GET_DESCRIPTOR
					use xhci::ring::trb::transfer::*;
					const GET_DESCRIPTOR: u8 = 6;
					const DESCRIPTOR_DEVICE: u16 = 1 << 8;
					const DESCRIPTOR_CONFIGURATION: u16 = 2 << 8;
					const DESCRIPTOR_STRING: u16 = 3 << 8;
					const DESCRIPTOR_DEVICE_QUALIFIER: u16 = 6 << 8;
					unsafe {
						tr_ptr.cast::<[u32; 4]>().add(tr_enqueue_i).write(
							SetupStage::new()
								.set_request_type(0b1000_0000)
								.set_transfer_type(TransferType::Out)
								.set_request(GET_DESCRIPTOR)
								.set_value(DESCRIPTOR_STRING | 1)
								.set_index(0)
								.set_length(64)
								.set_cycle_bit()
								.into_raw(),
						);
						tr_enqueue_i += 1;
						tr_ptr.cast::<[u32; 4]>().add(tr_enqueue_i).write(
							Isoch::new()
								.set_data_buffer_pointer(data_phys)
								// FIXME qemu crashes if this is less than length in SetupStage
								.set_trb_transfer_length(64)
								.set_chain_bit()
								.set_cycle_bit()
								.into_raw(),
						);
						tr_enqueue_i += 1;
						tr_ptr.cast::<[u32; 4]>().add(tr_enqueue_i).write(
							StatusStage::new()
								.set_cycle_bit()
								.set_interrupter_target(0)
								.set_interrupt_on_completion()
								.into_raw(),
						);
						tr_enqueue_i += 1;
						rt::dbg!(slot);
						regs.doorbell.update_at(slot.into(), |c| {
							c.set_doorbell_stream_id(0).set_doorbell_target(1);
						});
					}
					let len = unsafe { data_ptr.cast::<u8>().read_volatile() };
					let str_len = (len - 2) / 2;
					let s = unsafe {
						core::slice::from_raw_parts(data_ptr.cast::<u16>().add(1), str_len.into())
					};
					for c in char::decode_utf16(s.iter().copied()).map(Result::unwrap) {
						rt::dbg!(c);
					}
				}
			}
		}
	}

	/*
	for (i, p) in (&regs.port_register_set).into_iter().enumerate() {
		if p.portsc.port_enabled_disabled() {
			rt::dbg!(i, p);
		}
	}
	*/
	/*
		let pci = unsafe { pci::Pci::new(pci_config.cast(), 0, 0, &[]) };

		let mut dev = {
			let h = pci.get(0, 0, 0).unwrap();
			match h {
				pci::Header::H0(h) => {
					let map_bar = |bar: u8| {
						assert!(bar < 6);
						let mut s = *b"bar0";
						s[3] += bar;
						dev_handle
							.open(&s)
							.unwrap()
							.map_object(None, rt::io::RWX::RW, 0, usize::MAX)
							.unwrap()
							.0
							.cast()
					};
					let dma_alloc = |size: usize, _align| -> Result<_, ()> {
						let (d, a, _) = driver_utils::dma::alloc_dma(size.try_into().unwrap()).unwrap();
						Ok((d.cast(), virtio::PhysAddr::new(a.try_into().unwrap())))
					};

					let msix = virtio_block::Msix { queue: Some(0) };

					unsafe { virtio_block::BlockDevice::new(h, map_bar, dma_alloc, msix).unwrap() }
				}
				_ => unreachable!(),
			}
		};

		// Register new table of Streaming type
		let (tbl, dma_phys) = {
			let (dma, dma_phys) =
				driver_utils::dma::alloc_dma_object((1 << 16).try_into().unwrap()).unwrap();
			let tbl = StreamTable::new(&dma, Pow2Size(9), (1 << 12) - 1);
			file_root
				.create(table_name)
				.unwrap()
				.share(tbl.public())
				.unwrap();
			(tbl, dma_phys)
		};

		let mut data_handles = driver_utils::Arena::new();

		loop {
			let wait = || poll.read(&mut []).unwrap();

			let mut flush = false;
			while let Some((handle, req)) = tbl.dequeue() {
				let (job_id, resp) = match req {
					Request::Open { job_id, path } => {
						let r = if handle == Handle::MAX {
							if path.len() == 4 && {
								let mut buf = [0; 4];
								path.copy_to(0, &mut buf);
								buf == *b"data"
							} {
								Response::Handle(data_handles.insert(0))
							} else {
								Response::Error(rt::Error::InvalidData)
							}
						} else {
							Response::Error(rt::Error::InvalidOperation)
						};
						path.manual_drop();
						(job_id, r)
					}
					Request::Read { job_id, amount } => {
						(
							job_id,
							if handle == Handle::MAX {
								Response::Error(rt::Error::InvalidOperation)
							} else {
								// TODO how do we with unaligned reads/writes?
								assert!(amount % SECTOR_SIZE == 0);
								let amount = amount.min(1 << 13);
								let offset = data_handles[handle];

								let data = tbl
									.alloc(amount.try_into().unwrap())
									.expect("out of buffers");
								let sectors = data.blocks().map(|b| virtio::PhysRegion {
									base: virtio::PhysAddr::new(dma_phys + u64::from(b.0) * 512),
									size: 512,
								});

								let tk = unsafe { dev.read(sectors, offset).unwrap() };
								// TODO proper async
								while dev.poll_finished(|t| assert_eq!(t, tk)) != 1 {
									wait();
								}

								data_handles[handle] += u64::from(amount / SECTOR_SIZE);

								Response::Data(data)
							},
						)
					}
					Request::Write { job_id, data } => {
						// TODO ditto
						assert!(data.len() % Sector::SIZE == 0);
						let offset = data_handles[handle];

						let sectors = data.blocks().map(|b| virtio::PhysRegion {
							base: virtio::PhysAddr::new(dma_phys + u64::from(b.0) * 512),
							size: 512,
						});

						let tk = unsafe { dev.write(sectors, offset).unwrap() };
						// TODO proper async
						while dev.poll_finished(|t| assert_eq!(t, tk)) != 1 {
							wait();
						}
						let len = data.len();

						data.manual_drop();

						data_handles[handle] += u64::try_from(len / Sector::SIZE).unwrap();

						(job_id, Response::Amount(len.try_into().unwrap()))
					}
					Request::Seek { job_id, from } => {
						let offset = match from {
							rt::io::SeekFrom::Start(n) => n,
							_ => todo!(),
						};
						// TODO ditto
						assert!(offset % u64::from(SECTOR_SIZE) == 0);
						data_handles[handle] = offset / u64::from(SECTOR_SIZE);
						(job_id, Response::Position(offset))
					}
					Request::Close => {
						data_handles.remove(handle);
						// The kernel does not expect a response.
						continue;
					}
					Request::Create { job_id, path } => {
						path.manual_drop();
						(job_id, Response::Error(rt::Error::InvalidOperation))
					}
					Request::Share { .. } => todo!(),
					Request::GetMeta { .. } => todo!(),
					Request::SetMeta { .. } => todo!(),
					Request::Destroy { .. } => todo!(),
				};
				tbl.enqueue(job_id, resp);
				flush = true;
			}
			flush.then(|| tbl.flush());
			tbl.wait();
		}
	*/
	todo!()
}

/// Observed errata and workarounds.
pub struct Errata(u64);

macro_rules! errata {
	($err:ident $fn:ident $n:literal) => {
		const $err: u64 = 1 << $n;

		fn $fn(&self) -> bool {
			self.0 & Self::$err != 0
		}
	};
}

impl Errata {
	errata!(NO_PSCE_ON_RESET no_psce_on_reset 0);

	pub const NONE: Self = Self(0);
	pub const PCI_1B36_000D: Self = Self(Self::NO_PSCE_ON_RESET);
}

fn calc_packet_size(speed: u8) -> u16 {
	const FULL_SPEED: u8 = 1;
	const LOW_SPEED: u8 = 2;
	const HIGH_SPEED: u8 = 3;
	const SUPERSPEED_GEN1_X1: u8 = 4;
	const SUPERSPEED_GEN2_X1: u8 = 5;
	const SUPERSPEED_GEN1_X2: u8 = 6;
	const SUPERSPEED_GEN2_X2: u8 = 7;

	match speed {
		0 => panic!("uninitialized"),
		LOW_SPEED => 8,
		HIGH_SPEED => 64,
		SUPERSPEED_GEN1_X1 | SUPERSPEED_GEN2_X1 | SUPERSPEED_GEN1_X2 | SUPERSPEED_GEN2_X2 => 512,
		FULL_SPEED => todo!("use GET_DESCRIPTOR to get packet size"),
		n => unimplemented!("unknown speed {}", n),
	}
}
