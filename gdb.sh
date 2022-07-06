#!/bin/sh

. ./env.sh

gdb \
	-ex='target extended-remote localhost:1234' \
	"$@"
