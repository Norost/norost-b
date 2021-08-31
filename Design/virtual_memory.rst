==============
Virtual memory
==============


Address range management
~~~~~~~~~~~~~~~~~~~~~~~~

Processes are expected to maintain a list of used ranges themselves.
Regardless, the kernel will check if a process attempts to overwrite an
already-mapped region.


Page tables
~~~~~~~~~~~

Page tables are managed in kernel-space. The kernel can transparently support
hugepages (i.e. without the user process explicitly requesting it). Each page
is either:

* Directly mapped
* Private
* Shared
* Shared but with RWX flags locked
