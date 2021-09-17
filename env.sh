#!/bin/sh

SCRIPTPATH="$( cd -- "$(dirname "$0")" >/dev/null 2>&1 ; pwd -P )"

export CC=gcc
export CC_32=gcc
export RUST_TARGET="x86_64-unknown-norostb"
export RUST_TARGET_32="i686-unknown-norostb"
export RUST_TARGET_FILE="$SCRIPTPATH/$RUST_TARGET.json"
export RUST_TARGET_FILE_32="$SCRIPTPATH/$RUST_TARGET_32.json"
export ARCH=amd64
