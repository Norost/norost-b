use crate::dma::Dma;
use core::{char::DecodeUtf16Error, slice::ArrayChunks};

// https://wiki.osdev.org/USB#GET_DESCRIPTOR
const GET_DESCRIPTOR: u8 = 6;
const DESCRIPTOR_DEVICE: u8 = 1;
const DESCRIPTOR_CONFIGURATION: u8 = 2;
const DESCRIPTOR_STRING: u8 = 3;

const FULL_SPEED: u8 = 1;
const LOW_SPEED: u8 = 2;
const HIGH_SPEED: u8 = 3;
const SUPERSPEED_GEN1_X1: u8 = 4;
const SUPERSPEED_GEN2_X1: u8 = 5;
const SUPERSPEED_GEN1_X2: u8 = 6;
const SUPERSPEED_GEN2_X2: u8 = 7;

pub enum GetDescriptor {
	Device,
	Configuration { index: u8 },
	String { index: u8 },
}

pub enum DescriptorResult<'a> {
	Device(Device),
	Configuration(Configuration),
	String(DescriptorStringIter<'a>),
}

#[derive(Debug)]
// repr(C) so the compiler doesn't try to optimize layout and subsequently deoptimizes decode.
#[repr(C)]
pub struct Device {
	pub usb: u16,
	pub class: u8,
	pub subclass: u8,
	pub protocol: u8,
	pub max_packet_size_0: u8,
	pub vendor: u16,
	pub product: u16,
	pub device: u16,
	pub index_manufacturer: u8,
	pub index_product: u8,
	pub index_serial_number: u8,
	pub num_configurations: u8,
}

#[derive(Debug)]
// ditto
#[repr(C)]
pub struct Configuration {
	pub total_length: u16,
	pub num_interfaces: u8,
	pub configuration_value: u8,
	pub index_configuration: u8,
	pub attributes: ConfigurationAttributes,
	pub max_power: u8,
}

#[derive(Debug)]
pub struct ConfigurationAttributes(u8);

macro_rules! flag {
	($i:literal $f:ident) => {
		fn $f(&self) -> bool {
			self.0 & 1 << $i != 0
		}
	};
}

impl ConfigurationAttributes {
	flag!(6 self_powered);
	flag!(5 remote_wakeup);
}

pub struct DescriptorStringIter<'a>(ArrayChunks<'a, u8, 2>);

pub enum Request<'a> {
	GetDescriptor {
		ty: GetDescriptor,
		buffer: &'a Dma<[u8]>,
	},
}

pub struct RawRequest {
	pub request_type: u8,
	pub direction: Direction,
	pub request: u8,
	pub value: u16,
	pub index: u16,
	pub buffer_len: u16,
	pub buffer_phys: u64,
}

pub enum Direction {
	In,
	Out,
}

pub struct InvalidDescriptor;

impl Request<'_> {
	pub fn into_raw(self) -> RawRequest {
		match self {
			Self::GetDescriptor { ty, buffer } => RawRequest {
				request_type: 0b1000_0000,
				direction: Direction::Out,
				request: GET_DESCRIPTOR,
				value: match ty {
					GetDescriptor::Device => u16::from(DESCRIPTOR_DEVICE) << 8,
					GetDescriptor::Configuration { index } => {
						u16::from(DESCRIPTOR_CONFIGURATION) << 8 | u16::from(index)
					}
					GetDescriptor::String { index } => {
						u16::from(DESCRIPTOR_STRING) << 8 | u16::from(index)
					}
				},
				index: 0,
				buffer_len: buffer.len().try_into().unwrap_or(u16::MAX),
				buffer_phys: buffer.as_phys(),
			},
		}
	}
}

impl<'a> DescriptorResult<'a> {
	pub fn decode(buf: &'a [u8]) -> Result<Self, InvalidDescriptor> {
		let len = *buf.get(0).ok_or(InvalidDescriptor)?;
		let ty = *buf.get(1).ok_or(InvalidDescriptor)?;
		let buf = buf.get(2..len.into()).ok_or(InvalidDescriptor)?;
		match ty {
			DESCRIPTOR_DEVICE => decode_device(buf).map(Self::Device),
			DESCRIPTOR_CONFIGURATION => decode_configuration(buf).map(Self::Configuration),
			DESCRIPTOR_STRING => decode_string(buf).map(Self::String),
			i => Err(InvalidDescriptor),
		}
	}
}

impl Iterator for DescriptorStringIter<'_> {
	type Item = u16;

	fn next(&mut self) -> Option<Self::Item> {
		self.0.next().copied().map(u16::from_le_bytes)
	}
}

fn decode_device(buf: &[u8]) -> Result<Device, InvalidDescriptor> {
	if buf.len() != 16 {
		return Err(InvalidDescriptor);
	}
	let f1 = |i: usize| buf[i - 2];
	let f2 = |i: usize| u16::from_le_bytes(buf[i - 2..i].try_into().unwrap());
	Ok(Device {
		usb: f2(2),
		class: f1(4),
		subclass: f1(5),
		protocol: f1(6),
		max_packet_size_0: f1(7),
		vendor: f2(8),
		product: f2(10),
		device: f2(12),
		index_manufacturer: f1(14),
		index_product: f1(15),
		index_serial_number: f1(16),
		num_configurations: f1(17),
	})
}

fn decode_configuration(buf: &[u8]) -> Result<Configuration, InvalidDescriptor> {
	if buf.len() != 7 {
		return Err(InvalidDescriptor);
	}
	let f1 = |i: usize| buf[i - 2];
	let f2 = |i: usize| u16::from_le_bytes(buf[i - 2..i].try_into().unwrap());
	Ok(Configuration {
		total_length: f2(2),
		num_interfaces: f1(4),
		configuration_value: f1(5),
		index_configuration: f1(6),
		attributes: ConfigurationAttributes(f1(7)),
		max_power: f1(8),
	})
}

fn decode_string(buf: &[u8]) -> Result<DescriptorStringIter<'_>, InvalidDescriptor> {
	Ok(DescriptorStringIter(buf.array_chunks::<2>()))
}
