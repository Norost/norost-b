#!/bin/sh

./mkiso.sh $MKISO_ARGS || exit $?
make disk0 usb0 || exit

cpu="--enable-kvm -cpu host"

[ -z ${CPU+x} ] || cpu="-cpu $CPU"

exec qemu-system-x86_64 \
	$cpu \
	-m 256M \
	-machine q35 \
	-drive format=raw,media=cdrom,file=norost.iso \
	-drive file=disk0,format=raw,if=none,id=disk0 \
	-drive file=usb0,format=raw,if=none,id=usb0 \
	-serial mon:stdio \
	-device virtio-blk-pci,drive=disk0 \
	-netdev user,id=net0,hostfwd=tcp::5555-:80,hostfwd=tcp::2222-:22 \
	-device virtio-net-pci,netdev=net0 \
	-device qemu-xhci \
	-device usb-kbd \
	-device usb-storage,drive=usb0 \
	-vga virtio \
	-s \
	"$@"
	#-device usb-mouse \
