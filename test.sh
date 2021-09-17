#!/bin/sh

./mkiso.sh

echo "Checking noraboot"
if grub-file --is-x86-multiboot2 isodir/boot/noraboot; then
	echo "  Multiboot 2 OK" 
else
	echo "  Multiboot 2 header is invalid"
fi
