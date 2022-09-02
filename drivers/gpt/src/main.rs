//! ## References
//!
//! * https://wiki.osdev.org/GPT

#![no_std]
#![feature(start)]
#![feature(str_internals)]

extern crate alloc;

use {
	alloc::{
		string::{String, ToString},
		vec::Vec,
	},
	core::str,
	driver_utils::os::stream_table::{Request, Response, StreamTable},
	rt_default as _,
};

use core::{fmt, str::lossy::Utf8Lossy};

#[start]
fn start(_: isize, _: *const *const u8) -> isize {
	main();
	0
}

fn main() {
	let disk = rt::args::handle(b"data").expect("data object undefined");
	let share = rt::args::handle(b"share").expect("share object undefined");

	let mut disk = Controller::new(disk);

	let buf = disk.read(1);

	let header = PartitionTableHeader::try_from(&buf[..]).unwrap();
	assert!(
		header.partition_entry_count < 1 << 20,
		"todo: deal with huge partition count efficiently"
	);

	let mut partitions = Vec::new();

	for i in 0..header.partition_entry_count {
		let offt = u64::from(header.partition_entry_size) * u64::from(i);
		let lba = header.partition_entry_array_lba + offt / 512;
		let buf = disk.read(lba);
		let e = PartitionEntry::try_from(&buf[offt as usize % 512..]).unwrap();
		if e.is_used() {
			let i = i as usize;
			partitions.resize(i + 1, None);
			partitions[i] = Some((e.start_lba, e.end_lba));
		}
	}

	let (buf, _) = rt::Object::new(rt::NewObject::SharedMemory { size: 1 << 12 }).unwrap();
	let tbl = StreamTable::new(&buf, 512.try_into().unwrap(), 512 - 1);
	share.create(b"gpt").unwrap().share(tbl.public()).unwrap();

	let mut obj = driver_utils::Arena::new();
	let mut ls = driver_utils::Arena::new();
	let disk = disk.dev;

	loop {
		tbl.wait();
		let mut flush = false;
		while let Some((handle, job_id, req)) = tbl.dequeue() {
			let resp = match req {
				Request::Open { path } => {
					let mut buf = [0; 32];
					let (s, _) = path.copy_into(&mut buf);
					if s == b"" || s == b"/" {
						Response::Handle(ls.insert(0) | 1 << 31)
					} else if let Some(i) =
						str::from_utf8(s).ok().and_then(|s| s.parse::<usize>().ok())
					{
						if partitions.get(i).map_or(false, |e| e.is_some()) {
							Response::Handle(obj.insert((i, 0)))
						} else {
							Response::Error(rt::Error::DoesNotExist)
						}
					} else {
						Response::Error(rt::Error::InvalidData)
					}
				}
				Request::Read { amount } if handle != rt::Handle::MAX => {
					if handle & 1 << 31 != 0 {
						let i = &mut ls[handle ^ 1 << 31];
						let s = partitions
							.iter()
							.enumerate()
							.skip(*i)
							.find(|(_, e)| e.is_some())
							.map_or_else(String::new, |(k, _)| {
								*i = k + 1;
								k.to_string()
							});
						let buf = tbl.alloc(s.len()).unwrap();
						buf.copy_from(0, s.as_bytes());
						Response::Data(buf)
					} else if amount != 512 {
						Response::Error(rt::Error::InvalidData)
					} else {
						let (i, pos) = &mut obj[handle];
						let (start, end) = partitions[*i].unwrap();
						if *pos <= end - start {
							let buf = tbl.alloc(512).unwrap();
							disk.seek(rt::io::SeekFrom::Start((start + *pos) * 512))
								.unwrap();
							disk.read(unsafe { buf.blocks().next().unwrap().1.as_mut() })
								.unwrap();
							*pos += 1;
							Response::Data(buf)
						} else {
							Response::Error(rt::Error::InvalidData)
						}
					}
				}
				Request::Write { data } if handle & 1 << 31 == 0 => {
					if data.len() != 512 {
						Response::Error(rt::Error::InvalidData)
					} else {
						let (i, pos) = &mut obj[handle];
						let (start, end) = partitions[*i].unwrap();
						if *pos <= end - start {
							let (_, b) = data.blocks().next().unwrap();
							disk.seek(rt::io::SeekFrom::Start((start + *pos) * 512))
								.unwrap();
							disk.write(unsafe { b.as_ref() }.try_into().unwrap())
								.unwrap();
							*pos += 1;
							Response::Amount(512)
						} else {
							Response::Error(rt::Error::InvalidData)
						}
					}
				}
				Request::Seek { from } if handle & 1 << 31 == 0 => match from {
					rt::io::SeekFrom::Start(n) if n % 512 == 0 => {
						let (i, pos) = &mut obj[handle];
						let (start, end) = partitions[*i].unwrap();
						*pos = (n / 512).min(end - start);
						Response::Position(n)
					}
					_ => Response::Error(rt::Error::InvalidData),
				},
				Request::Close => {
					if handle != rt::Handle::MAX {
						if handle & 1 << 31 != 0 {
							ls.remove(handle ^ 1 << 31).unwrap();
						} else {
							obj.remove(handle).unwrap();
						}
					}
					continue;
				}
				_ => Response::Error(rt::Error::InvalidOperation),
			};
			tbl.enqueue(job_id, resp);
			flush = true;
		}
		flush.then(|| tbl.flush());
	}
}

#[derive(Debug)]
struct PartitionTableHeader {
	#[allow(dead_code)]
	gpt_revision: u32,
	#[allow(dead_code)]
	header_size: u32,
	#[allow(dead_code)]
	crc32: u32,
	#[allow(dead_code)]
	header_lba: u64,
	#[allow(dead_code)]
	alt_header_lba: u64,
	#[allow(dead_code)]
	first_usable_block: u64,
	#[allow(dead_code)]
	last_usable_block: u64,
	#[allow(dead_code)]
	guid: u128,
	partition_entry_array_lba: u64,
	partition_entry_count: u32,
	partition_entry_size: u32,
	#[allow(dead_code)]
	partition_entry_array_crc32: u32,
}

impl PartitionTableHeader {
	const SIGNATURE: [u8; 8] = *b"EFI PART";
}

impl TryFrom<&[u8]> for PartitionTableHeader {
	type Error = InvalidPartitionTableHeader;

	fn try_from(a: &[u8]) -> Result<Self, Self::Error> {
		if a.len() < 0x80 {
			return Err(InvalidPartitionTableHeader::TooShort);
		}
		if &a[..8] != &Self::SIGNATURE {
			return Err(InvalidPartitionTableHeader::InvalidSignature);
		}
		let f4 = |i| u32::from_le_bytes(a[i..][..4].try_into().unwrap());
		let f8 = |i| u64::from_le_bytes(a[i..][..8].try_into().unwrap());
		let f16 = |i| u128::from_le_bytes(a[i..][..16].try_into().unwrap());
		Ok(Self {
			gpt_revision: f4(0x8),
			header_size: f4(0xc),
			crc32: f4(0x10),
			header_lba: f8(0x18),
			alt_header_lba: f8(0x20),
			first_usable_block: f8(0x28),
			last_usable_block: f8(0x30),
			guid: f16(0x38),
			partition_entry_array_lba: f8(0x48),
			partition_entry_count: f4(0x50),
			partition_entry_size: f4(0x54),
			partition_entry_array_crc32: f4(0x58),
		})
	}
}

#[derive(Debug)]
enum InvalidPartitionTableHeader {
	InvalidSignature,
	TooShort,
}

struct PartitionEntry {
	type_guid: u128,
	partition_guid: u128,
	start_lba: u64,
	end_lba: u64,
	attributes: u64,
	partition_name: [u8; 72],
}

impl PartitionEntry {
	fn is_used(&self) -> bool {
		self.type_guid != 0
	}
}

impl TryFrom<&[u8]> for PartitionEntry {
	type Error = InvalidPartitionEntry;

	fn try_from(a: &[u8]) -> Result<Self, Self::Error> {
		if a.len() < 0x80 {
			return Err(InvalidPartitionEntry::TooShort);
		}
		let f8 = |i| u64::from_le_bytes(a[i..][..8].try_into().unwrap());
		let f16 = |i| u128::from_le_bytes(a[i..][..16].try_into().unwrap());
		Ok(Self {
			type_guid: f16(0x0),
			partition_guid: f16(0x10),
			start_lba: f8(0x20),
			end_lba: f8(0x28),
			attributes: f8(0x30),
			// FIXME Actually UTF-16
			partition_name: a[0x38..][..72].try_into().unwrap(),
		})
	}
}

impl fmt::Debug for PartitionEntry {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct(stringify!(PartitionEntry))
			.field("type_guid", &format_args!("{:032x}", self.type_guid))
			.field(
				"partition_guid",
				&format_args!("{:032x}", self.partition_guid),
			)
			.field("start_lba", &self.start_lba)
			.field("end_lba", &self.end_lba)
			.field("attributes", &self.attributes)
			.field(
				"partition_name",
				&Utf8Lossy::from_bytes(&self.partition_name),
			)
			.finish()
	}
}

#[derive(Debug)]
enum InvalidPartitionEntry {
	TooShort,
}

struct Controller {
	dev: rt::RefObject<'static>,
	cache: [u8; 512],
	cache_pos: u64,
}

impl Controller {
	fn new(dev: rt::RefObject<'static>) -> Self {
		Self { dev, cache: [0; 512], cache_pos: u64::MAX }
	}

	fn read(&mut self, pos: u64) -> &[u8; 512] {
		if self.cache_pos != pos {
			self.dev.seek(rt::io::SeekFrom::Start(pos * 512)).unwrap();
			self.dev.read(&mut self.cache).unwrap();
			self.cache_pos = pos;
		}
		&self.cache
	}
}
