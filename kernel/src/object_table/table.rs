use super::{Error, JobId, JobResult, JobTask};
use alloc::sync::Arc;
use core::time::Duration;

/// A table of objects.
pub trait Table {
	fn take_job(self: Arc<Self>, _timeout: Duration) -> JobTask {
		unimplemented!()
	}

	fn finish_job(
		self: Arc<Self>,
		_job: Result<JobResult, Error>,
		_job_id: JobId,
	) -> Result<(), ()> {
		unimplemented!()
	}

	fn cancel_job(self: Arc<Self>, _job_id: JobId) {
		unimplemented!()
	}
}
