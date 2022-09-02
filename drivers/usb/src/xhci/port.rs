use {
	super::{Pending, Xhci},
	core::num::NonZeroU8,
	xhci::ring::trb::{
		command::{Allowed, DisableSlot, EnableSlot},
		event::PortStatusChange,
	},
};

impl Xhci {
	pub(super) fn handle_port_status_change(&mut self, trb: PortStatusChange) {
		let port = trb.port_id();
		trace!("handle port change, port {}", port);
		let i = (port - 1).into();
		let port = NonZeroU8::new(port).expect("port 0 received port status change event");
		let mut prs = self.registers.port_register_set.port_register_set_mut(i);
		let mut portsc = prs.portsc.read_volatile();

		// Make sure we don't accidently disable the port
		portsc.unset_port_enabled_disabled();
		prs.portsc.write_volatile(portsc);

		if !portsc.connect_status_change() && !portsc.port_reset_change() {
			trace!("not connect status or port reset change, ignore");
			return;
		}

		if !portsc.current_connect_status() {
			trace!("port is not connected");
			if let Some(slot) = self.port_slot_map[usize::from(port.get() - 1)].take() {
				self.disable_slot(slot);
			}
			return;
		}

		// Reset if USB2
		// TODO check if USB2 via extended capabilities
		if !portsc.port_reset_change() {
			trace!("reset port");
			portsc.set_port_reset();
			prs.portsc.write_volatile(portsc);
			return;
		}

		let port_speed = portsc.port_speed();

		info!("enable slot for port {}", port);
		self.enqueue_command(
			Allowed::EnableSlot(*EnableSlot::new().set_slot_type(0)),
			Pending::AllocSlot { port, port_speed },
		);
	}

	fn disable_slot(&mut self, slot: NonZeroU8) {
		info!("disable slot {}", slot);
		self.enqueue_command(
			Allowed::DisableSlot(*DisableSlot::new().set_slot_id(slot.get())),
			Pending::DeallocSlot { slot },
		);
	}

	/// # Safety
	///
	/// The slot must not be in use by the controller.
	pub unsafe fn dealloc_slot(&mut self, slot: NonZeroU8) {
		trace!("dealloc slot {}", slot);
		self.dcbaa.set(slot, 0);
		self.devices.remove(&slot).expect("no device at slot");
	}
}
