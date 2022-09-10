use {
	alloc::vec::Vec,
	core::{mem, ops::RangeInclusive},
	usb_hid_item::{Field, Value},
};

#[derive(Debug)]
pub struct Report {
	pub fields: Vec<(Usages, Field)>,
}

impl Report {
	/// The size of the report in bytes
	pub fn size(&self) -> u32 {
		self.fields
			.iter()
			.map(|(_, f)| f.report_count as u32 * f.report_size as u32)
			.sum::<u32>()
			/ 8
	}
}

#[derive(Debug, Default)]
pub struct Usages(Vec<(u16, RangeInclusive<u16>)>);

impl Usages {
	/// Get the usage at the given index.
	pub fn get(&self, mut index: u32) -> Option<(u16, u16)> {
		for (page, usages) in &self.0 {
			if let Some(i) = index.checked_sub(usages.len() as _) {
				index = i;
			} else {
				return usages.clone().nth(index as _).map(|id| (*page, id));
			}
		}
		None
	}

	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}
}

pub fn parse(data: &[u8]) -> Report {
	let mut report = Report { fields: Vec::new() };
	let mut usages = Usages(Vec::new());

	fn f(val: Value, report: &mut Report, usages: &mut Usages) {
		match val {
			Value::Collection(_) | Value::EndCollection => usages.0.clear(),
			Value::Usage { page, ids } => usages.0.push((page, ids)),
			Value::Field(f) => report.fields.push((mem::take(usages), f)),
			Value::StackFrame(s) => s.for_each(|e| f(e.unwrap(), report, usages)),
		}
	}
	usb_hid_item::parse(data)
		.iter()
		.for_each(|e| f(e.unwrap(), &mut report, &mut usages));
	report
}
