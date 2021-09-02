doc: doc-html

doc-html: build/doc/design.html


build/doc/design.html: Design/Index.rst | build/doc/
	rst2html5.py $< $@


build/doc/: build/
	mkdir build/doc

build/:
	mkdir build/

