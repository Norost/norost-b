#!/bin/sh

./mkiso.sh || exit $?
make disk0 || exit $?

cpu="--enable-kvm -cpu host"

[ -z ${CPU+x} ] || cpu="-cpu $CPU"

qemu-system-x86_64 \
	$cpu \
	-drive format=raw,file=norost.iso \
	-serial mon:stdio $@ \
	-machine q35 \
	-drive file=disk0,format=raw,if=none,id=disk0 \
	-device virtio-blk-pci,drive=disk0 \
	#-s
