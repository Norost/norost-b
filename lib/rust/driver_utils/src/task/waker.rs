use core::task::{RawWaker, RawWakerVTable, Waker};

static DUMMY_VTABLE: RawWakerVTable = RawWakerVTable::new(
	|_| RawWaker::new(0 as _, &DUMMY_VTABLE),
	|_| (),
	|_| (),
	|_| (),
);

pub fn dummy() -> Waker {
	// SAFETY: the waker does literally nothing.
	unsafe { Waker::from_raw(RawWaker::new(0 as _, &DUMMY_VTABLE)) }
}
