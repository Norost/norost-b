#!/bin/sh

./mkiso.sh || exit $?

qemu-system-x86_64 --enable-kvm -cpu host -drive format=raw,file=norost.iso
