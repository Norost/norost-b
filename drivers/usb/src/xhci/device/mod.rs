//mod init;

use {
	super::{ring, DeviceConfig, Pending, Xhci},
	crate::dma::Dma,
	alloc::{boxed::Box, vec::Vec},
	core::num::NonZeroU8,
	usb_request::{
		descriptor::{Direction, EndpointTransfer},
		RawRequest,
	},
	xhci::{
		context::{Device32Byte, EndpointState, EndpointType, Input32Byte, InputHandler},
		ring::trb::{command, transfer},
	},
};

const FULL_SPEED: u8 = 1;
const LOW_SPEED: u8 = 2;
const HIGH_SPEED: u8 = 3;
const SUPERSPEED_GEN1_X1: u8 = 4;
const SUPERSPEED_GEN2_X1: u8 = 5;
const SUPERSPEED_GEN1_X2: u8 = 6;
const SUPERSPEED_GEN2_X2: u8 = 7;

pub(super) struct Device {
	slot: NonZeroU8,
	port: NonZeroU8,
	port_speed: u8,
	_output_dev_context: Dma<Device32Byte>,
	transfer_ring: ring::Ring<transfer::Allowed>,
	endpoints: Box<[Option<ring::Ring<transfer::Normal>>]>,
}

impl Device {
	pub fn send_request(
		&mut self,
		interrupter: u16,
		req: &RawRequest,
		buf: &Dma<[u8]>,
	) -> Result<ring::EntryId, ()> {
		// Setup
		let len = u16::try_from(buf.len()).unwrap_or(u16::MAX);
		trace!(
			"send request ty {:#x} val {:#x} index {:#x} phys {:#x} len {}",
			req.request_type,
			req.value,
			req.index,
			buf.as_phys(),
			buf.len()
		);
		let ring = &mut self.transfer_ring;

		let (dir, trf) = if req.direction_in() {
			(transfer::Direction::In, transfer::TransferType::In)
		} else {
			(transfer::Direction::Out, transfer::TransferType::Out)
		};

		// Page 85 of manual for example
		ring.enqueue(transfer::Allowed::SetupStage(
			*transfer::SetupStage::new()
				.set_request_type(req.request_type)
				.set_transfer_type(trf)
				.set_request(req.request)
				.set_value(req.value)
				.set_index(req.index)
				.set_length(len),
		));
		// Data
		if len > 0 {
			ring.enqueue(transfer::Allowed::DataStage(
				*transfer::DataStage::new()
					.set_direction(dir)
					.set_data_buffer_pointer(buf.as_phys())
					// FIXME qemu crashes if this is less than length in SetupStage
					.set_trb_transfer_length(len.into()),
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
	) -> Result<ring::EntryId, TransferError> {
		trace!(
			"transfer ep {} intr {:?} phys {:#x} len {}",
			endpoint,
			interrupter,
			data.as_phys(),
			data.len()
		);
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
			.get_mut(usize::from(endpoint).wrapping_sub(2))
			.and_then(|o| o.as_mut())
			.ok_or(TransferError::InvalidEndpoint { endpoint })?
			.enqueue(xfer);
		Ok(id)
	}

	pub fn configure(&mut self, config: DeviceConfig<'_>) -> (command::Allowed, Dma<Input32Byte>) {
		trace!("configure device slot {}", self.slot);
		let mut input_context = Dma::<Input32Byte>::new().unwrap_or_else(|_| todo!());
		let inp = unsafe { input_context.as_mut() };

		let c = inp.control_mut();
		c.set_configuration_value(config.config.configuration_value);
		assert_eq!(config.interface.number, 0, "todo");
		c.set_interface_number(config.interface.number);
		assert_eq!(config.interface.alternate_setting, 0, "todo");
		c.set_alternate_setting(config.interface.alternate_setting);

		let mut max_dci = 0;
		let mut endpoints = Vec::new();
		for ep_descr in config.endpoints {
			trace!("enable {:?}", ep_descr);
			let index = usize::from(ep_descr.address.number()) * 2
				+ match ep_descr.address.direction() {
					Direction::Out => 0,
					Direction::In => 1,
				};

			let l = self.endpoints.len().max(index - 1);
			endpoints.resize_with(l, || None);
			assert!(
				endpoints[index - 2].is_none(),
				"endpoint already initialized"
			);

			let ring = ring::Ring::new().unwrap_or_else(|_| todo!());

			// 4.8.2.4
			let ep = inp.device_mut().endpoint_mut(index);
			ep.set_endpoint_state(EndpointState::Running);
			ep.set_endpoint_type(map_endpoint_type(
				ep_descr.attributes.transfer(),
				ep_descr.address.direction(),
			));
			ep.set_max_packet_size(ep_descr.max_packet_size);
			ep.set_max_burst_size(0);
			ep.set_tr_dequeue_pointer(ring.as_phys());
			ep.set_dequeue_cycle_state();
			ep.set_interval(ep_descr.interval);
			ep.set_max_primary_streams(0);
			ep.set_mult(0);
			ep.set_error_count(3);

			endpoints[index - 2] = Some(ring);

			inp.control_mut().set_add_context_flag(index);
			max_dci = max_dci.max(index as _);
		}

		let sl = inp.device_mut().slot_mut();
		sl.set_root_hub_port_number(self.port.get());
		sl.set_speed(self.port_speed);
		sl.set_context_entries(max_dci);
		inp.control_mut().set_add_context_flag(0);

		self.endpoints = endpoints.into();
		let cmd = *command::ConfigureEndpoint::new()
			.set_input_context_pointer(input_context.as_phys())
			.set_slot_id(self.slot.get());
		(command::Allowed::ConfigureEndpoint(cmd), input_context)
	}

	pub fn slot(&self) -> NonZeroU8 {
		self.slot
	}
}

pub enum TransferError {
	InvalidEndpoint { endpoint: u8 },
}

impl Xhci {
	pub(super) fn set_address(&mut self, port: NonZeroU8, slot: NonZeroU8, port_speed: u8) {
		trace!(
			"set address port {} slot {} speed {}",
			port,
			slot,
			port_speed
		);
		// Allocate an Input Context
		let mut input_context = Dma::<Input32Byte>::new().unwrap_or_else(|_| todo!());
		let input = unsafe { input_context.as_mut() };

		// Set A0, A1
		input.control_mut().set_add_context_flag(0);
		input.control_mut().set_add_context_flag(1);

		// Initialize the Input Slot Context
		// FIXME how? what's the topology?
		let sl = input.device_mut().slot_mut();
		sl.set_root_hub_port_number(port.get());
		sl.set_context_entries(1);
		sl.set_speed(port_speed);

		// Allocate and initialize the Transfer Ring for the Default Control Endpoint
		let transfer_ring = ring::Ring::new().unwrap_or_else(|_| todo!());

		let (pkt_size, adjust_packet_size) = calc_packet_size(port_speed);
		trace!(
			"packet size {}, needs adjust: {}",
			pkt_size,
			adjust_packet_size
		);

		// Initialize the Input default control Endpoint 0 Context
		let ep = input.device_mut().endpoint_mut(1);
		ep.set_endpoint_type(EndpointType::Control);
		ep.set_max_packet_size(pkt_size);
		ep.set_tr_dequeue_pointer(transfer_ring.as_phys());
		ep.set_dequeue_cycle_state();
		ep.set_error_count(3);

		// Allocate the Output Device Context data structure and set to '0'
		let _output_dev_context = Dma::<Device32Byte>::new().unwrap_or_else(|_| todo!());

		// Load the appropriate (Device Slot ID) entry in the Device Context Base Address Array
		self.dcbaa.set(slot, _output_dev_context.as_phys());

		// Issue an Address Device Command for the Device Slot
		self.enqueue_command(
			command::Allowed::AddressDevice(
				*command::AddressDevice::new()
					.set_slot_id(slot.get())
					.set_input_context_pointer(input_context.as_phys()),
			),
			Pending::SetAddress(SetAddress {
				dev: Device {
					slot,
					port,
					port_speed,
					_output_dev_context,
					transfer_ring,
					endpoints: Default::default(),
				},
				input_context,
				adjust_packet_size,
			}),
		);
	}
}

pub(super) struct SetAddress {
	pub(super) dev: Device,
	adjust_packet_size: bool,
	input_context: Dma<Input32Byte>,
}

impl SetAddress {
	pub fn should_adjust_packet_size(&self) -> bool {
		self.adjust_packet_size
	}

	pub fn adjust_packet_size(&mut self, pkt_size: u8) -> command::Allowed {
		trace!("adjust packet size");
		assert!(self.adjust_packet_size);
		self.adjust_packet_size = false;

		let inp = unsafe { self.input_context.as_mut() };

		inp.control_mut().set_add_context_flag(1);

		let ep = inp.device_mut().endpoint_mut(1);
		ep.set_endpoint_type(EndpointType::Control);
		ep.set_max_packet_size(pkt_size.into());
		ep.set_max_burst_size(0);
		ep.set_tr_dequeue_pointer(self.dev.transfer_ring.as_phys());
		ep.set_dequeue_cycle_state();
		ep.set_error_count(3);
		inp.control_mut().set_add_context_flag(1); // evaluate ep 1 context

		let cmd = *command::EvaluateContext::new()
			.set_input_context_pointer(self.input_context.as_phys())
			.set_slot_id(self.dev.slot.get());
		command::Allowed::EvaluateContext(cmd)
	}

	#[must_use]
	pub fn finish(self) -> Device {
		trace!("finish set address");
		assert!(!self.adjust_packet_size);
		self.dev
	}
}

/// The boolean field indicates whether GET_DESCRIPTOR should be used to get the real packet size.
///
/// # Note
///
/// The speed must come from the **link** register.
fn calc_packet_size(speed: u8) -> (u16, bool) {
	match speed {
		0 => panic!("uninitialized"),
		LOW_SPEED => (8, false),
		HIGH_SPEED => (64, false),
		SUPERSPEED_GEN1_X1 | SUPERSPEED_GEN2_X1 | SUPERSPEED_GEN1_X2 | SUPERSPEED_GEN2_X2 => {
			(512, false)
		}
		FULL_SPEED => (8, true),
		n => unimplemented!("unknown speed {}", n),
	}
}

fn map_endpoint_type(transfer: EndpointTransfer, dir: Direction) -> EndpointType {
	match (transfer, dir) {
		(EndpointTransfer::Interrupt, Direction::In) => EndpointType::InterruptIn,
		(EndpointTransfer::Bulk, Direction::In) => EndpointType::BulkIn,
		(EndpointTransfer::Bulk, Direction::Out) => EndpointType::BulkOut,
		e => todo!("{:?}", e),
	}
}
