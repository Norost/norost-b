use super::{RegR, RegRW, RegW};
use crate::memory::frame::PPN;
use crate::memory::r#virtual::{phys_to_virt, AddressSpace};
use core::fmt;

#[repr(C, align(4096))]
pub struct LocalApic {
	_reserved_0_1: [RegR; 0x1 - 0x0 + 1],
	pub id: RegR,
	pub version: RegR,
	_reserved_4_7: [RegR; 0x7 - 0x4 + 1],
	pub task_priority: RegRW,
	pub arbitration_priority: RegR,

	pub processor_priority: RegR,
	pub eoi: RegW,
	pub remote_read: RegR,
	pub logical_destination: RegRW,
	pub destination_format: RegRW,
	pub spurious_interrupt_vector: RegRW,

	pub in_service: [RegR; 8],
	pub trigger_mode: [RegR; 8],

	pub interrupt_request: [RegR; 8],
	pub error_status: RegR,
	_reserved_29_2e: [RegR; 0x2e - 0x29 + 1],
	pub lvt_cmci: RegRW,

	pub interrupt_command: [RegRW; 2],
	pub lvt_timer: RegRW,
	pub lvt_thermal_sensor: RegRW,
	pub lvt_performance_monitoring_counters: RegRW,
	pub lvt_lint0: RegRW,
	pub lvt_lint1: RegRW,
	pub lvt_error: RegRW,
	pub initial_count: RegRW,
	pub current_count: RegR,
	_reserved_3a_3d: [RegR; 0x3d - 0x3a + 1],
	pub divide_configuration: RegRW,
}

impl LocalApic {
	pub fn in_service(&self) -> BitSet256 {
		reg_to_bitset(&self.in_service)
	}

	pub fn trigger_mode(&self) -> BitSet256 {
		reg_to_bitset(&self.trigger_mode)
	}

	pub fn interrupt_request(&self) -> BitSet256 {
		reg_to_bitset(&self.interrupt_request)
	}
}

fn reg_to_bitset(regs: &[RegR; 8]) -> BitSet256 {
	let mut set = [0; 2];
	// TODO don't use transmute
	let v = unsafe { core::mem::transmute::<_, &mut [u32; 8]>(&mut set) };
	regs.iter()
		.zip(v.iter_mut())
		.for_each(|(r, w)| *w = r.get());
	BitSet256(set)
}

impl fmt::Debug for LocalApic {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		use core::fmt::DebugStruct as DS;
		let mut f = f.debug_struct(stringify!(LocalApic));
		let hex = |f: &mut DS, n, v: u32| {
			f.field(n, &format_args!("{:#x}", v));
		};
		let set = |f: &mut DS, n, v: BitSet256| {
			f.field(n, &format_args!("{:?}", v));
		};

		hex(&mut f, "id", self.id.get());
		hex(&mut f, "version", self.version.get());
		hex(&mut f, "task_priority", self.task_priority.get());
		hex(
			&mut f,
			"arbitration_priority",
			self.arbitration_priority.get(),
		);
		hex(&mut f, "processor_priority", self.processor_priority.get());
		// FIXME QEMU sets ESR even though it shouldn't
		// See https://www.intel.com/content/dam/www/public/us/en/documents/manuals/64-ia-32-architectures-so&mut ftware-developer-vol-3a-part-1-manual.pdf
		//hex(&mut f, "remote_read",  self.remote_read.get());
		hex(
			&mut f,
			"logical_destination",
			self.logical_destination.get(),
		);
		hex(&mut f, "destination_format", self.destination_format.get());
		hex(
			&mut f,
			"spurious_interrupt_vector",
			self.spurious_interrupt_vector.get(),
		);
		set(&mut f, "in_service", self.in_service());
		set(&mut f, "trigger_mode", self.trigger_mode());
		set(&mut f, "interrupt_request", self.interrupt_request());
		hex(&mut f, "error_status", self.error_status.get());
		// FIXME ditto
		//hex(&mut f, "lvt_cmci",  self.lvt_cmci.get());
		//hex(&mut f, "interrupt_command",  self.interrupt_command.get());
		hex(&mut f, "lvt_timer", self.lvt_timer.get());
		hex(&mut f, "lvt_thermal_sensor", self.lvt_thermal_sensor.get());
		hex(
			&mut f,
			"lvt_performance_monitoring_counters",
			self.lvt_performance_monitoring_counters.get(),
		);
		hex(&mut f, "lvt_lint0", self.lvt_lint0.get());
		hex(&mut f, "lvt_lint1", self.lvt_lint1.get());
		hex(&mut f, "lvt_error", self.lvt_error.get());
		hex(&mut f, "initial_count", self.initial_count.get());
		hex(&mut f, "current_count", self.current_count.get());
		hex(
			&mut f,
			"divide_configuration",
			self.divide_configuration.get(),
		);

		f.finish()
	}
}

pub struct BitSet256([u128; 2]);

impl fmt::Debug for BitSet256 {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		let mut f = f.debug_set();
		let mut p = |i, v, o| {
			if v & (1u128 << i) > 0u128 {
				let i = i + o;
				f.entry(&i);
			}
		};
		(0..128).for_each(|i| p(i, self.0[0], 0));
		(0..128).for_each(|i| p(i, self.0[1], 128));
		f.finish()
	}
}

pub fn get() -> &'static LocalApic {
	// SAFETY: The local APIC is always present.
	unsafe { &*(phys_to_virt(super::local_apic_address()).cast()) }
}

pub(super) fn init() {
	// Ensure the LAPIC registers are mapped.
	let a = PPN::try_from_usize(super::local_apic_address().try_into().unwrap()).unwrap();
	AddressSpace::identity_map(a, 4096);
	super::enable_apic();
}
