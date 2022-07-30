/// An identity accessor mapper
#[derive(Clone, Copy, Debug)]
pub struct Identity;

impl accessor::Mapper for Identity {
	unsafe fn map(&mut self, phys_start: usize, _bytes: usize) -> core::num::NonZeroUsize {
		phys_start.try_into().unwrap()
	}

	fn unmap(&mut self, _virt_start: usize, _bytes: usize) {}
}
