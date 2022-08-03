#!/bin/bash
set -e

source ../enqt/tfds-master/toolchain-netztester2.sh
make LLVM=1 ARCH=arm64 CROSS_COMPILE=aarch64-buildroot-linux-gnu- -j32 "$@"
