#!/bin/sh

set -e
set -x

. ./env.sh

cd kernel
cargo rustc "$@" -- \
	-C link-arg=-Tkernel/src/arch/$ARCH/link.ld \
	-C link-arg=kernel/src/arch/$ARCH/idt.s \
	-C linker=$CC \
	-C link-arg=-nostartfiles \
	-C no-redzone=yes \
	-Z unstable-options \
	-C split-debuginfo=unpacked
