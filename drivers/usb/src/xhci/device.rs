use super::{event::Event, ring, Xhci};
use crate::dma::Dma;
use core::{marker::PhantomData, mem, num::NonZeroU8, ptr::NonNull};
use driver_utils::dma;
use xhci::{
	accessor::Mapper,
	context::{self, InputHandler},
	ring::trb::{command, event, transfer},
	Registers,
};

// https://wiki.osdev.org/USB#GET_DESCRIPTOR
const GET_DESCRIPTOR: u8 = 6;
const DESCRIPTOR_DEVICE: u16 = 1 << 8;
const DESCRIPTOR_CONFIGURATION: u16 = 2 << 8;
const DESCRIPTOR_STRING: u16 = 3 << 8;
const DESCRIPTOR_DEVICE_QUALIFIER: u16 = 6 << 8;

const FULL_SPEED: u8 = 1;
const LOW_SPEED: u8 = 2;
const HIGH_SPEED: u8 = 3;
const SUPERSPEED_GEN1_X1: u8 = 4;
const SUPERSPEED_GEN2_X1: u8 = 5;
const SUPERSPEED_GEN1_X2: u8 = 6;
const SUPERSPEED_GEN2_X2: u8 = 7;

pub struct Device {
	port: NonZeroU8,
	slot: NonZeroU8,
	transfer_ring: ring::Ring<transfer::Allowed>,
	input_context: Dma<context::Input32Byte>,
	output_dev_context: Dma<context::Device32Byte>,
}

impl Device {
	pub fn test(&mut self, ctrl: &mut Xhci) {
		rt::dbg!(self.slot, self.port);
		// software will issue USB GET_DESCRIPTOR requests
		let mut tr_enqueue_i = 0;
		let (data_ptr, data_phys, _) =
			driver_utils::dma::alloc_dma(64.try_into().unwrap()).unwrap();
		self.transfer_ring.enqueue(transfer::Allowed::SetupStage(
			*transfer::SetupStage::new()
				.set_request_type(0b1000_0000)
				.set_transfer_type(transfer::TransferType::Out)
				.set_request(GET_DESCRIPTOR)
				.set_value(DESCRIPTOR_STRING | 1)
				.set_index(0)
				.set_length(64),
		));
		self.transfer_ring.enqueue(transfer::Allowed::Isoch(
			*transfer::Isoch::new()
				.set_data_buffer_pointer(data_phys)
				// FIXME qemu crashes if this is less than length in SetupStage
				.set_trb_transfer_length(64)
				.set_chain_bit(),
		));
		self.transfer_ring.enqueue(transfer::Allowed::StatusStage(
			*transfer::StatusStage::new()
				.set_interrupter_target(0)
				.set_interrupt_on_completion(),
		));
		rt::dbg!();
		ctrl.registers
			.doorbell
			.update_volatile_at(self.slot.get().into(), |c| {
				c.set_doorbell_stream_id(0).set_doorbell_target(1);
			});
		rt::dbg!();
		let len = unsafe { data_ptr.cast::<u8>().as_ptr().read_volatile() };
		let str_len = (len - 2) / 2;
		let s = unsafe {
			core::slice::from_raw_parts(data_ptr.cast::<u16>().as_ptr().add(1), str_len.into())
		};
		for c in char::decode_utf16(s.iter().copied()) {
			rt::dbg!(c);
		}
	}
}

pub(super) fn init(port: NonZeroU8, ctrl: &mut Xhci) -> Result<WaitReset, &'static str> {
	// Reset if USB2
	// TODO check if USB2
	ctrl.registers
		.port_register_set
		.update_volatile_at((port.get() - 1).into(), |c| {
			c.portsc.set_port_reset();
		});
	Ok(WaitReset { port })
}

#[must_use]
pub struct WaitReset {
	port: NonZeroU8,
}

impl WaitReset {
	pub fn poll(
		&mut self,
		ctrl: &mut Xhci,
	) -> Option<Result<(ring::EntryId, AllocSlot), ring::Full>> {
		ctrl.registers
			.port_register_set
			.read_volatile_at((self.port.get() - 1).into())
			.portsc
			.port_reset_change()
			.then(|| {
				// system software shall obtain a Device Slot
				let e = ctrl.command_ring.enqueue(command::Allowed::EnableSlot(
					*command::EnableSlot::new().set_slot_type(0),
				))?;
				ctrl.registers.doorbell.update_volatile_at(0, |c| {
					c.set_doorbell_stream_id(0).set_doorbell_target(0);
				});
				Ok((e, AllocSlot { port: self.port }))
			})
	}
}

#[must_use]
pub struct AllocSlot {
	port: NonZeroU8,
}

impl AllocSlot {
	pub fn init(
		&mut self,
		ctrl: &mut Xhci,
		slot: NonZeroU8,
	) -> Result<(ring::EntryId, SetAddress), ring::Full> {
		// Allocate an Input Context
		let mut input_context = Dma::<context::Input32Byte>::new().unwrap_or_else(|_| todo!());
		let input = unsafe { input_context.as_mut() };

		// Set A0, A1
		input.control_mut().set_add_context_flag(0);
		input.control_mut().set_add_context_flag(1);

		// Initialize the Input Slot Context
		// FIXME how? what's the topology?
		input
			.device_mut()
			.slot_mut()
			.set_root_hub_port_number(slot.get());
		//input.device_mut().slot_mut().set_route_string(todo!());
		input.device_mut().slot_mut().set_context_entries(1);

		// Allocate and initialize the Transfer Ring for the Default Control Endpoint
		let transfer_ring = ring::Ring::new().unwrap_or_else(|_| todo!());

		// Initialize the Input default control Endpoint 0 Context
		let ep = input.device_mut().endpoint_mut(1);
		ep.set_endpoint_type(xhci::context::EndpointType::Control);
		ep.set_max_packet_size(calc_packet_size(
			ctrl.registers
				.port_register_set
				.read_volatile_at((self.port.get() - 1).into())
				.portsc
				.port_speed(),
		));
		ep.set_max_burst_size(0);
		ep.set_tr_dequeue_pointer(transfer_ring.as_phys());
		ep.set_dequeue_cycle_state();
		ep.set_interval(0);
		ep.set_max_primary_streams(0);
		ep.set_mult(0);
		ep.set_error_count(0);

		// Allocate the Output Device Context data structure and set to '0'
		let output_dev_context = Dma::<context::Device32Byte>::new().unwrap_or_else(|_| todo!());

		// Load the appropriate (Device Slot ID) entry in the Device Context Base Address Array
		ctrl.dcbaap.set(slot.into(), output_dev_context.as_phys());

		// Issue an Address Device Command for the Device Slot
		rt::dbg!();
		let e = ctrl.enqueue_command(command::Allowed::AddressDevice(
			*command::AddressDevice::new()
				.set_slot_id(slot.get())
				.set_input_context_pointer(input_context.as_phys()),
		))?;
		rt::dbg!();
		Ok((
			e,
			SetAddress {
				dev: Device {
					port: self.port,
					slot,
					transfer_ring,
					input_context,
					output_dev_context,
				},
			},
		))
	}
}

pub struct SetAddress {
	dev: Device,
}

impl SetAddress {
	pub fn finish(self) -> Device {
		self.dev
	}
}

fn calc_packet_size(speed: u8) -> u16 {
	match speed {
		0 => panic!("uninitialized"),
		LOW_SPEED => 8,
		HIGH_SPEED => 64,
		SUPERSPEED_GEN1_X1 | SUPERSPEED_GEN2_X1 | SUPERSPEED_GEN1_X2 | SUPERSPEED_GEN2_X2 => 512,
		FULL_SPEED => todo!("use GET_DESCRIPTOR to get packet size"),
		n => unimplemented!("unknown speed {}", n),
	}
}
