pub struct Executor {
	_scratchpad: [usize; 2],
	active_thread: NonNull<Thread>,
}
