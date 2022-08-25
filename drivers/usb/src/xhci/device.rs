use super::{event::Event, ring, DeviceConfig, Xhci};
use crate::{
	dma::Dma,
	requests::{
		Direction, EndpointAttributes, EndpointTransfer, EndpointUsage, RawRequest, Request,
	},
};
use alloc::vec::Vec;
use core::{marker::PhantomData, mem, num::NonZeroU8, ptr::NonNull};
use driver_utils::dma;
use xhci::{
	accessor::Mapper,
	context::{
		Device32Byte, DeviceHandler, EndpointHandler, EndpointState, EndpointType, Input32Byte,
		InputHandler,
	},
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

pub(super) struct Device {
	port: NonZeroU8,
	slot: NonZeroU8,
	output_dev_context: Dma<Device32Byte>,
	transfer_ring: ring::Ring<transfer::Allowed>,
	endpoints: Vec<Option<ring::Ring<transfer::Normal>>>,
}

impl Device {
	pub fn send_request(
		&mut self,
		interrupter: u16,
		req: &RawRequest,
	) -> Result<ring::EntryId, ring::Full> {
		// Setup
		let (phys, len) = req
			.buffer
			.as_ref()
			.map_or((0, 0), |b| (b.as_phys(), b.len()));
		let len = u16::try_from(len).unwrap_or(u16::MAX);
		let ring = &mut self.transfer_ring;
		ring.enqueue(transfer::Allowed::SetupStage(
			*transfer::SetupStage::new()
				.set_request_type(req.request_type)
				.set_transfer_type(transfer::TransferType::Out)
				.set_request(req.request)
				.set_value(req.value)
				.set_index(req.index)
				.set_length(len),
		));
		// Data
		if len > 0 {
			ring.enqueue(transfer::Allowed::Isoch(
				// FIXME Isoch doesn't make sense, does it?
				*transfer::Isoch::new()
					.set_data_buffer_pointer(phys)
					// FIXME qemu crashes if this is less than length in SetupStage
					.set_trb_transfer_length(len.into())
					.set_chain_bit(),
			));
		}
		// Status
		let id = ring.enqueue(transfer::Allowed::StatusStage(
			*transfer::StatusStage::new()
				.set_interrupter_target(interrupter)
				.set_interrupt_on_completion(),
		));
		Ok(id)
	}

	pub fn transfer(
		&mut self,
		endpoint: u8,
		interrupter: Option<u16>,
		data: &Dma<[u8]>,
	) -> Result<ring::EntryId, ring::Full> {
		let mut xfer = transfer::Normal::new();
		xfer.set_data_buffer_pointer(data.as_phys())
			.set_trb_transfer_length(data.len().try_into().expect("data too large"))
			.set_td_size(0); // the amount of packets to be sent after, so I guess 0? (TODO)
		if let Some(intr) = interrupter {
			xfer.set_interrupter_target(intr)
				.set_interrupt_on_completion();
		}
		let id = self
			.endpoints
			.get_mut(usize::from(endpoint) - 2)
			.and_then(|o| o.as_mut())
			.expect("invalid/unitinialized endpoint")
			.enqueue(xfer);
		Ok(id)
	}

	pub fn configure(
		&mut self,
		interrupter: u16,
		config: DeviceConfig,
	) -> (command::Allowed, Dma<Input32Byte>) {
		let mut input_context = Dma::<Input32Byte>::new().unwrap_or_else(|_| todo!());
		let inp = unsafe { input_context.as_mut() };

		inp.control_mut().set_add_context_flag(0); // evaluate slot context

		for ep_descr in config.endpoints {
			let index = usize::from(ep_descr.address.number()) * 2
				+ match ep_descr.address.direction() {
					Direction::Out => 0,
					Direction::In => 1,
				};

			let l = self.endpoints.len().max(index - 1);
			self.endpoints.resize_with(l, || None);
			assert!(
				self.endpoints[index - 2].is_none(),
				"endpoint already initialized"
			);

			let ring = ring::Ring::new().unwrap_or_else(|_| todo!());

			// 4.8.2.4
			let ep = inp.device_mut().endpoint_mut(index);
			ep.set_endpoint_type(map_endpoint_type(
				ep_descr.attributes.transfer(),
				ep_descr.address.direction(),
			));
			ep.set_max_packet_size(ep_descr.max_packet_size);
			ep.set_max_burst_size(0);
			ep.set_tr_dequeue_pointer(ring.as_phys());
			ep.set_dequeue_cycle_state();
			ep.set_interval(0);
			ep.set_max_primary_streams(0);
			ep.set_mult(0);
			ep.set_error_count(3);
			ep.set_interval(8); // 128µs * 8 = 1ms (TODO set this properly)

			self.endpoints[index - 2] = Some(ring);

			inp.control_mut().set_add_context_flag(index); // evaluate endpoint context
		}
		let cmd = *command::ConfigureEndpoint::new()
			.set_input_context_pointer(input_context.as_phys())
			.set_slot_id(self.slot.get());
		(command::Allowed::ConfigureEndpoint(cmd), input_context)
	}

	pub fn transfer_in(&mut self, interrupter: u16) {}

	pub fn slot(&self) -> NonZeroU8 {
		self.slot
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
pub(super) struct WaitReset {
	port: NonZeroU8,
}

impl WaitReset {
	pub fn poll(
		&mut self,
		regs: &mut xhci::Registers<impl Mapper + Clone>,
	) -> Option<(command::Allowed, AllocSlot)> {
		regs.port_register_set
			.read_volatile_at((self.port.get() - 1).into())
			.portsc
			.port_reset_change()
			.then(|| {
				// system software shall obtain a Device Slot
				(
					command::Allowed::EnableSlot(*command::EnableSlot::new().set_slot_type(0)),
					AllocSlot { port: self.port },
				)
			})
	}
}

#[must_use]
pub(super) struct AllocSlot {
	port: NonZeroU8,
}

impl AllocSlot {
	pub fn init(
		&mut self,
		ctrl: &mut Xhci,
		//regs: &mut xhci::Registers<impl Mapper + Clone>,
		slot: NonZeroU8,
	) -> Result<(ring::EntryId, SetAddress), ring::Full> {
		// Allocate an Input Context
		let mut input_context = Dma::<Input32Byte>::new().unwrap_or_else(|_| todo!());
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
		ep.set_endpoint_type(EndpointType::Control);
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
		let output_dev_context = Dma::<Device32Byte>::new().unwrap_or_else(|_| todo!());

		// Load the appropriate (Device Slot ID) entry in the Device Context Base Address Array
		ctrl.dcbaap.set(slot.into(), output_dev_context.as_phys());

		// Issue an Address Device Command for the Device Slot
		Ok((
			ctrl.enqueue_command(command::Allowed::AddressDevice(
				*command::AddressDevice::new()
					.set_slot_id(slot.get())
					.set_input_context_pointer(input_context.as_phys()),
			))?,
			SetAddress {
				dev: Device {
					port: self.port,
					slot,
					output_dev_context,
					transfer_ring,
					endpoints: Default::default(),
				},
			},
		))
	}
}

#[must_use]
pub(super) struct SetAddress {
	dev: Device,
}

impl SetAddress {
	#[must_use]
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

fn map_endpoint_type(transfer: EndpointTransfer, dir: Direction) -> EndpointType {
	rt::dbg!(match (transfer, dir) {
		(EndpointTransfer::Interrupt, Direction::In) => EndpointType::InterruptIn,
		(EndpointTransfer::Bulk, Direction::In) => EndpointType::BulkIn,
		(EndpointTransfer::Bulk, Direction::Out) => EndpointType::BulkOut,
		e => todo!("{:?}", e),
	})
}
