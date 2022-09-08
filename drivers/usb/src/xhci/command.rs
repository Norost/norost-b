use {
	super::{device, ring::EntryId, Event, Xhci},
	crate::dma::Dma,
	core::num::NonZeroU8,
	xhci::ring::trb::{
		command::Allowed,
		event::{CommandCompletion, CompletionCode},
	},
};

pub(super) enum Pending {
	AllocSlot { port: NonZeroU8, port_speed: u8 },
	DeallocSlot { slot: NonZeroU8 },
	SetAddress(device::SetAddress),
	ConfigureDev,
}

impl Xhci {
	pub(super) fn enqueue_command(&mut self, cmd: Allowed, pending: Pending) -> EntryId {
		trace!("enqueue command {:?}", &cmd);
		let id = self.command_ring.enqueue(cmd);
		trace!("id {:#x}", id);
		self.registers.doorbell.update_volatile_at(0, |c| {
			c.set_doorbell_stream_id(0).set_doorbell_target(0);
		});
		self.pending.insert(id, pending);
		id
	}

	pub(super) fn handle_pending(&mut self, event: CommandCompletion) -> Option<Event> {
		let slot = event.slot_id();
		let code = event.completion_code();
		let id = event.command_trb_pointer();
		trace!("handle pending id {:x} slot {}, {:?}", id, slot, code);

		match self
			.pending
			.remove(&id)
			.expect("no pending command with id")
		{
			Pending::AllocSlot { port, port_speed } => {
				assert_eq!(code, Ok(CompletionCode::Success));
				let slot = NonZeroU8::new(slot).expect("AllocSlot for slot 0");
				self.port_slot_map[usize::from(port.get() - 1)] = Some(slot);
				self.set_address(port, slot, port_speed);
				None
			}
			Pending::SetAddress(mut e) => {
				assert_eq!(code, Ok(CompletionCode::Success));
				if e.should_adjust_packet_size() {
					trace!("adjust packet size: get descriptor");
					let req = usb_request::Request::GetDescriptor {
						ty: usb_request::descriptor::GetDescriptor::Device,
					}
					.into_raw();
					let buf = Dma::new_slice(8).unwrap();
					let id = e
						.dev
						.send_request(0, &req, &buf)
						.unwrap_or_else(|_| todo!());
					self.ring(e.dev.slot().get(), 0, 1);
					self.transfers_config_packet_size.insert(id, (e, buf));
					return None;
				}
				trace!("finish set address, configure device");
				let d = e.finish();
				let d = self
					.devices
					.entry(d.slot())
					.and_modify(|_| panic!("slot already occupied"))
					.or_insert(d);
				Some(Event::NewDevice { slot: d.slot() })
			}
			Pending::ConfigureDev => {
				let slot = NonZeroU8::new(slot).expect("ConfigureDev for slot 0");
				Some(Event::DeviceConfigured { id, slot, code })
			}
			Pending::DeallocSlot { slot } => {
				// SAFETY: the controller has just deallocated the slot.
				unsafe { self.dealloc_slot(slot) }
				None
			}
		}
	}
}
