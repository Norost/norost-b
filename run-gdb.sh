#!/bin/sh

./mkiso.sh || exit $?
make disk0 || exit $?

#cpu="--enable-kvm -cpu host"

[ -z ${CPU+x} ] || cpu="-cpu $CPU"

gdb --args qemu-system-x86_64 \
	$cpu \
	-drive format=raw,file=norost.iso \
	-machine q35 \
	-drive file=disk0,format=raw,if=none,id=disk0 \
	-device virtio-blk-pci,drive=disk0 \
	-netdev user,id=net0,hostfwd=tcp::5555-:80,hostfwd=tcp::2222-:22 \
	-device virtio-net-pci,netdev=net0 \
	-device qemu-xhci \
	-device usb-kbd \
	"$@"
