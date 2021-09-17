#!/bin/sh

./mkiso.sh || exit $?

qemu-system-x86_64 -drive format=raw,file=norost.iso
