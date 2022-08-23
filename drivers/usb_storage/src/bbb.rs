//! # Bulk-Bulk-Bulk (BBB) transaction utilities.
//!
//! Each transaction is performed via 3 bulk transfers:
//!
//! * The first is the command which is always sent to Bulk Out.
//! * The second is either an input or output buffer sent via Bulk In or Out respectively.
//! * The last is a status buffer which is always sent to Bulk In.
//!
//! Only one transfer can be in progress at any time.
//!
//! ## References
//!
//! [Mass Storage Bulk Only 1.0](https://www.usb.org/sites/default/files/usbmassbulk_10.pdf)

// TODO report QEMU crash:
// qemu-system-x86_64: ../hw/usb/dev-storage.c:228: usb_msd_transfer_data: Assertion `(s->mode == USB_MSDM_DATAOUT) == (req->cmd.mode == SCSI_XFER_TO_DEV)' failed.
//
// I would do it myself if Gitlab wasn't user-hostile *shrug*.

pub struct Device<'a> {
	data_out: ipc_usb::Endpoint,
	data_in: ipc_usb::Endpoint,
	wr: rt::RefObject<'a>,
	rd: rt::RefObject<'a>,
}

impl<'a> Device<'a> {
	pub fn new(
		data_out: ipc_usb::Endpoint,
		data_in: ipc_usb::Endpoint,
		wr: &'a rt::Object,
		rd: &'a rt::Object,
	) -> Self {
		Self {
			data_out,
			data_in,
			wr: wr.into(),
			rd: rd.into(),
		}
	}

	/// Perform a BBB Out transfer
	pub fn transfer_out(
		&mut self,
		command: impl scsi::Command,
		data: &[u8],
	) -> Result<u32, rt::Error> {
		// CBW
		let mut cmd = [0; 16];
		let cmd_len = command.into_raw(&mut cmd).len();
		let data_len = data.len().try_into().expect("data exceeds 4GB");
		self.transfer_command(0x00, cmd, cmd_len, data_len)?;

		// Data
		if !data.is_empty() {
			ipc_usb::send_data_out(ipc_usb::Endpoint::N2, |d| self.wr.write(d))?;
			self.wr.write(data)?;
		}

		// CSW
		self.transfer_status(data_len)
	}

	/// Perform a BBB Out transfer
	pub fn transfer_in(
		&mut self,
		command: impl scsi::Command,
		length: u32,
	) -> Result<alloc::vec::Vec<u8>, rt::Error> {
		// CBW
		let mut cmd = [0; 16];
		let len = command.into_raw(&mut cmd).len();
		self.transfer_command(0x80, cmd, len, length)?;

		// Data
		if length != 0 {
			ipc_usb::send_data_in(self.data_in, length, |d| self.wr.write(d))?;
		}
		let mut buf = alloc::vec::Vec::with_capacity((32 + length) as _);
		let l = self.rd.read_uninit(buf.spare_capacity_mut())?.0.len();
		// SAFETY: read_uninit guarantees the first l bytes are initialized.
		unsafe { buf.set_len(l) }
		buf.drain(..2);

		// CSW
		let sl = self.transfer_status(length)?;
		//assert_eq!(l, sl as usize);

		Ok(buf)
	}

	fn transfer_command(
		&mut self,
		flags: u8,
		cmd: [u8; 16],
		cmd_len: usize,
		data_transfer_length: u32,
	) -> Result<(), rt::Error> {
		let cmd = CommandBlockWrapper {
			tag: 0,
			data_transfer_length,
			flags,
			lun: 0,
			cb_length: cmd_len.try_into().unwrap(),
			data: cmd,
		};
		ipc_usb::send_data_out(self.data_out, |d| self.wr.write(d))?;
		self.wr.write(&cmd.into_raw())?;
		Ok(())
	}

	fn transfer_status(&mut self, data_len: u32) -> Result<u32, rt::Error> {
		ipc_usb::send_data_in(self.data_in, 13, |d| self.wr.write(d))?;
		let mut buf = [0; 32];
		let l = self.rd.read(&mut buf)?;
		match ipc_usb::recv_parse(&buf[..l]).unwrap() {
			ipc_usb::Recv::DataIn { ep, data } => {
				let csw = CommandStatusWrapper::from_raw(data.try_into().unwrap());
				assert!(matches!(csw.status, Status::Success));
				Ok(data_len - csw.residue)
			}
		}
	}
}

struct CommandBlockWrapper {
	pub tag: u32,
	pub data_transfer_length: u32,
	pub flags: u8,
	pub lun: u8,
	pub cb_length: u8,
	pub data: [u8; 16],
}

impl CommandBlockWrapper {
	fn into_raw(self) -> [u8; 31] {
		let mut b = [0; 31];
		b[0..4].copy_from_slice(b"USBC"); // Took me way too long to realize
								  // to_ne_bytes() is fine since the tag is process-local
		b[4..8].copy_from_slice(&self.tag.to_ne_bytes());
		b[8..12].copy_from_slice(&self.data_transfer_length.to_le_bytes());
		b[12] = self.flags;
		b[13] = self.lun;
		b[14] = self.cb_length;
		b[15..].copy_from_slice(&self.data);
		b
	}
}

struct CommandStatusWrapper {
	pub residue: u32,
	pub status: Status,
}

impl CommandStatusWrapper {
	fn from_raw(raw: [u8; 13]) -> Self {
		assert!(&raw[..8] == b"USBS\0\0\0\0");
		Self {
			residue: u32::from_ne_bytes(raw[8..12].try_into().unwrap()),
			status: match raw[12] {
				0 => Status::Success,
				1 => Status::Failed,
				2 => Status::PhaseError,
				s => panic!("invalid status {}", s),
			},
		}
	}
}

pub enum Status {
	Success,
	Failed,
	PhaseError,
}
