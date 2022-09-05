pub struct LossyRingBuffer<T> {
	push: u8,
	pop: u8,
	data: [T; 128],
}

impl<T: Default + Copy> Default for LossyRingBuffer<T> {
	fn default() -> Self {
		Self { push: 0, pop: 0, data: [Default::default(); 128] }
	}
}

impl<T: Copy> LossyRingBuffer<T> {
	pub fn push(&mut self, item: T) {
		self.data[usize::from(self.push & 0x7f)] = item;
		let np = self.push.wrapping_add(1);
		if np ^ 128 != self.pop {
			self.push = np;
		}
	}

	pub fn pop(&mut self) -> Option<T> {
		(self.pop != self.push).then(|| {
			let item = self.data[usize::from(self.pop & 0x7f)];
			self.pop = self.pop.wrapping_add(1);
			item
		})
	}
}
