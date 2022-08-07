use crate::dma::Dma;
use core::{char::DecodeUtf16Error, mem, slice::ArrayChunks};

// https://wiki.osdev.org/USB#GET_DESCRIPTOR
const GET_DESCRIPTOR: u8 = 6;
const DESCRIPTOR_DEVICE: u8 = 1;
const DESCRIPTOR_CONFIGURATION: u8 = 2;
const DESCRIPTOR_STRING: u8 = 3;
const DESCRIPTOR_INTERFACE: u8 = 4;
const DESCRIPTOR_ENDPOINT: u8 = 5;
const DESCRIPTOR_DEVICE_QUALIFIER: u8 = 6;
const DESCRIPTOR_OTHER_SPEED_CONFIGURATION: u8 = 7;
const DESCRIPTOR_INTERFACE_POWER: u8 = 8;

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
	Interface(Interface),
	Endpoint(Endpoint),
	Unknown { ty: u8, data: &'a [u8] },
	Truncated { length: u8 },
	Invalid,
}

impl<'a> DescriptorResult<'a> {
	pub fn into_device(self) -> Option<Device> {
		match self {
			Self::Device(v) => Some(v),
			_ => None,
		}
	}

	pub fn into_configuration(self) -> Option<Configuration> {
		match self {
			Self::Configuration(v) => Some(v),
			_ => None,
		}
	}

	pub fn into_string(self) -> Option<DescriptorStringIter<'a>> {
		match self {
			Self::String(v) => Some(v),
			_ => None,
		}
	}
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

impl Configuration {
	const SIZE: usize = 2 + 7;
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

#[derive(Debug)]
// ditto
#[repr(C)]
pub struct Interface {
	pub number: u8,
	pub alternate_setting: u8,
	pub num_endpoints: u8,
	pub class: u8,
	pub subclass: u8,
	pub protocol: u8,
	pub index: u8,
}

impl Interface {
	const SIZE: usize = 2 + 7;
}

#[derive(Debug)]
// ditto
#[repr(C)]
pub struct Endpoint {
	pub endpoint_address: u8,
	pub attributes: u8,
	pub max_packet_size: u16,
	pub interval: u8,
}

impl Endpoint {
	const SIZE: usize = 2 + 5;
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
	pub fn decode(buf: &'a [u8]) -> Self {
		decode(buf).next().unwrap_or(Self::Invalid)
	}
}

impl Iterator for DescriptorStringIter<'_> {
	type Item = u16;

	fn next(&mut self) -> Option<Self::Item> {
		self.0.next().copied().map(u16::from_le_bytes)
	}

	fn size_hint(&self) -> (usize, Option<usize>) {
		(self.len(), Some(self.len()))
	}
}

impl ExactSizeIterator for DescriptorStringIter<'_> {
	fn len(&self) -> usize {
		self.0.len()
	}
}

fn decode_device(buf: &[u8]) -> Device {
	let f1 = |i: usize| buf[i - 2];
	let f2 = |i: usize| u16::from_le_bytes(buf[i - 2..i].try_into().unwrap());
	Device {
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
	}
}

fn decode_configuration<'a>(buf: &'a [u8]) -> Configuration {
	let f1 = |i: usize| buf[i - 2];
	let f2 = |i: usize| u16::from_le_bytes(buf[i - 2..i].try_into().unwrap());
	let num_interfaces = f1(4);
	Configuration {
		total_length: f2(2),
		num_interfaces,
		configuration_value: f1(5),
		index_configuration: f1(6),
		attributes: ConfigurationAttributes(f1(7)),
		max_power: f1(8),
	}
}

fn decode_string(buf: &[u8]) -> DescriptorStringIter<'_> {
	DescriptorStringIter(buf.array_chunks::<2>())
}

fn decode_interface(buf: &[u8]) -> Interface {
	let f1 = |i: usize| buf[i - 2];
	let num_endpoints = f1(4);
	Interface {
		number: f1(2),
		alternate_setting: f1(3),
		num_endpoints,
		class: f1(5),
		subclass: f1(6),
		protocol: f1(7),
		index: f1(8),
	}
}

fn decode_endpoint(buf: &[u8]) -> Endpoint {
	let f1 = |i: usize| buf[i - 2];
	let f2 = |i: usize| u16::from_le_bytes(buf[i - 2..i].try_into().unwrap());
	Endpoint {
		endpoint_address: f1(2),
		attributes: f1(3),
		max_packet_size: f2(4),
		interval: f1(6),
	}
}

pub fn decode(buf: &[u8]) -> Iter<'_> {
	Iter { buf }
}

pub struct Iter<'a> {
	buf: &'a [u8],
}

impl<'a> Iterator for Iter<'a> {
	type Item = DescriptorResult<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		(!self.buf.is_empty()).then(|| {
			let buf = mem::take(&mut self.buf);
			let l = buf[0];
			if l < 2 {
				return DescriptorResult::Invalid;
			}
			if usize::from(l) > buf.len() {
				return DescriptorResult::Truncated { length: l };
			}
			let b = &buf[2..usize::from(l)];
			let r = match buf[1] {
				DESCRIPTOR_DEVICE => DescriptorResult::Device(decode_device(b)),
				DESCRIPTOR_CONFIGURATION => {
					DescriptorResult::Configuration(decode_configuration(b))
				}
				DESCRIPTOR_STRING => DescriptorResult::String(decode_string(b)),
				DESCRIPTOR_INTERFACE => DescriptorResult::Interface(decode_interface(b)),
				DESCRIPTOR_ENDPOINT => DescriptorResult::Endpoint(decode_endpoint(b)),
				ty => DescriptorResult::Unknown { ty, data: b },
			};
			self.buf = &buf[usize::from(l)..];
			r
		})
	}
}
