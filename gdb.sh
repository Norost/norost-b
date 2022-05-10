#!/bin/sh

. ./env.sh

gdb \
	-ex='target extended-remote localhost:1234' \
	"target/x86_64-unknown-none-norostbkernel/debug/nora"
	#"target/x86_64-unknown-norostb/debug/driver_virtio_net"
