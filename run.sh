#!/bin/sh

./mkiso.sh $MKISO_ARGS || exit $?
make disk0 usb0 || exit

cpu="--enable-kvm -cpu host"

[ -z ${CPU+x} ] || cpu="-cpu $CPU"

exec qemu-system-x86_64 \
	$cpu \
	-m 256M \
	-machine q35 \
	-serial mon:stdio \
	-drive format=raw,media=cdrom,file=norost.iso \
	-drive file=disk0,format=raw,if=none,id=disk0 \
	-drive file=usb0,format=raw,if=none,id=usb0 \
	-device virtio-blk-pci,drive=disk0 \
	-netdev user,id=net0,hostfwd=tcp::5555-:80,hostfwd=tcp::2222-:22 \
	-device virtio-net-pci,netdev=net0 \
	-device qemu-xhci,p2=6,p3=0 \
	-device usb-tablet \
	-device usb-storage,drive=usb0,id=usbmsd \
	-vga virtio \
	-s \
	"$@"
