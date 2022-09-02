pub mod pvclock;

use {super::cpuid, crate::boot};

pub fn init(boot: &boot::Info, features: &cpuid::Features) {
	pvclock::init(boot, features);
}
