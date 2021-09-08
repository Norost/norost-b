include ../../common.mk

OUTPUT = $(shell pwd)/output
BUILD  = build

JOBS ?= $(shell nproc)

BINUTILS_VERSION = 2.37
GCC_VERSION      = 11.1.0

LIB_DIR     = $(SYSROOT)/lib
INCLUDE_DIR = $(SYSROOT)/include

build: gcc

install-dependencies-debian:
	sudo apt install \
		build-essential \
		bison flex \
		libgmp3-dev \
		libmpc-dev \
		libmpfr-dev \
		texinfo \
		libisl-dev


binutils: binutils-$(BINUTILS_VERSION) $(BUILD)/binutils
	cd $(BUILD)/binutils && \
		../../binutils-$(BINUTILS_VERSION)/configure \
		--target="$(TARGET)" \
		--prefix="$(OUTPUT)" \
		--with-sysroot="$(SYSROOT)" \
		--disable-nls \
		--disable-werror
	make -j$(JOBS) -C $(BUILD)/binutils
	make -j$(JOBS) -C $(BUILD)/binutils install

gcc: binutils gcc-$(GCC_VERSION) $(BUILD)/gcc | $(INCLUDE_DIR)
	cd $(BUILD)/gcc && \
		../../gcc-$(GCC_VERSION)/configure \
		--target="$(TARGET)" \
		--prefix="$(OUTPUT)" \
		--disable-nls \
		--enable-languages=c \
		--libdir="$(LIB_DIR)" \
		--includedir="$(INCLUDE_DIR)" \
		--with-arch=rv64imafdc \
		--program-prefix=$(TARGET)- \
		--target=$(TARGET) \
		--with-sysroot="$(SYSROOT)" \
		--with-as=$(OUTPUT)/bin/$(TARGET)-as \
		--with-ld=$(OUTPUT)/bin/$(TARGET)-ld \
		--with-build-time-tools=$(OUTPUT)/bin \
		--libexecdir=$(LIB_DIR)
	make -j$(JOBS) -C $(BUILD)/gcc all-gcc
	make -j$(JOBS) -C $(BUILD)/gcc all-target-libgcc
	make -j$(JOBS) -C $(BUILD)/gcc install-gcc
	make -j$(JOBS) -C $(BUILD)/gcc install-target-libgcc


binutils-$(BINUTILS_VERSION).tar.xz:
	curl https://ftp.gnu.org/gnu/binutils/binutils-$(BINUTILS_VERSION).tar.xz -O

gcc-$(GCC_VERSION).tar.xz:
	curl https://ftp.gnu.org/gnu/gcc/gcc-$(GCC_VERSION)/gcc-$(GCC_VERSION).tar.xz -O


binutils-$(BINUTILS_VERSION): binutils-$(BINUTILS_VERSION).tar.xz
	tar xJvf $<
	cd $@ && patch -p1 < ../patch/binutils-$(BINUTILS_VERSION).diff

gcc-$(GCC_VERSION): gcc-$(GCC_VERSION).tar.xz
	tar xJvf $<
	cd $@ && patch -p1 < ../patch/gcc-$(GCC_VERSION).diff

$(OUTPUT) $(INCLUDE_DIR):
	mkdir -p $@

$(BUILD)/%:
	mkdir -p $@

create-patches:
	tar xJvf binutils-$(BINUTILS_VERSION).tar.xz -C /tmp
	diff -ru /tmp/binutils-$(BINUTILS_VERSION) binutils-$(BINUTILS_VERSION) | grep -v ^O > patch/binutils-$(BINUTILS_VERSION).diff
	diff -ruN /tmp/binutils-$(BINUTILS_VERSION)/ld/emulparams binutils-$(BINUTILS_VERSION)/ld/emulparams >> patch/binutils-$(BINUTILS_VERSION).diff || true
	rm -r /tmp/binutils-$(BINUTILS_VERSION)
	tar xJvf gcc-$(GCC_VERSION).tar.xz -C /tmp
	diff -ru /tmp/gcc-$(GCC_VERSION) gcc-$(GCC_VERSION) --exclude='configure' | grep -v ^O > patch/gcc-$(GCC_VERSION).diff
	diff -ruN /tmp/gcc-$(GCC_VERSION)/gcc/config gcc-$(GCC_VERSION)/gcc/config >> patch/gcc-$(GCC_VERSION).diff || true
	rm -r /tmp/gcc-$(GCC_VERSION)


clean: clean-build clean-unpacked

distclean: clean-build clean-archives clean-unpacked clean-all

clean-build:
	rm -rf $(BUILD)

clean-archives:
	rm -f gcc-$(GCC_VERSION).tar.xz
	rm -f binutils-$(BINUTILS_VERSION).tar.xz

clean-unpacked:
	rm -rf gcc-$(GCC_VERSION)
	rm -rf binutils-$(BINUTILS_VERSION)

clean-all: clean
	rm -rf $(OUTPUT)

clean-all-local: clean-build clean-unpacked
