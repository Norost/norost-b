#!/bin/sh

./mkiso.sh || exit $?

cpu="--enable-kvm -cpu host"

[ -z ${CPU+x} ] || cpu="-cpu $CPU"

qemu-system-x86_64 $cpu -drive format=raw,file=norost.iso -serial mon:stdio $@
