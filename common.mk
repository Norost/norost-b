TOOLS_DIR = $(_common_mk_dir)thirdparty/tools/gcc/output/bin
TARGET    = riscv64-pc-norostb
SYSROOT   = $(_common_mk_dir)sysroot/$(TARGET)

CARGO_TARGET     = riscv64gc-unknown-none-elf
CARGO_OUTPUT_DIR = $(_common_mk_dir)target/$(CARGO_TARGET)/release
CARGO_PROFILE    = release
export CARGO_BUILD_TARGET = riscv64gc-unknown-none-elf

RUSTC_TARGET = $(_common_mk_dir)x86_64-unknown-norostb.json

CC        = $(TOOLS_DIR)/$(TARGET)-gcc
AR        = $(TOOLS_DIR)/$(TARGET)-ar
AS        = $(TOOLS_DIR)/$(TARGET)-as
STRIP     = $(TOOLS_DIR)/$(TARGET)-strip
READELF   = $(TOOLS_DIR)/$(TARGET)-readelf
OBJDUMP   = $(TOOLS_DIR)/$(TARGET)-objdump

_common_mk_dir = $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

default: build

$(SYSROOT):
	mkdir -p $@
