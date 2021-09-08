include ../../common.mk

HEADERS  = $(shell find include -type f -name '*.h')
DIRECTORIES = $(shell for n in `find src -type d -printf "%P\n"`; do if [ $$n ]; then echo "$(BUILD)/$$n"; fi; done)

HEADER_DIRS = $(patsubst %,$(SYSROOT)/%,$(shell for f in $(HEADERS); do echo "$${f%/*}"; done | sort | uniq))


build: install-headers

install-headers: $(patsubst %,$(SYSROOT)/%,$(HEADERS))

format:
	for f in $(HEADERS); do indent -linux -par "$$f"; done

$(SYSROOT)/include/%.h: include/%.h | $(HEADER_DIRS)
	cp $< $@

$(BUILD) $(DIRECTORIES) $(HEADER_DIRS):
	mkdir -p $@
