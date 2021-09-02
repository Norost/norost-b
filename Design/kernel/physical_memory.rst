===============
Physical memory
===============

Physical memory is managed by the kernel.


Backing store
~~~~~~~~~~~~~

Each individual page is tracked in a bitmap. To support hugepages and improve
lookup speeds, integers keep track of the amount of free pages in a range.

To reduce lookup cost yet make optimal use of space, each integer is an
unsigned byte for the first level and can track up to 256 pages. For the second
level, two bytes are used and hence can track 65536 pages, etc.

The layout for a 3 level table look like this::

   | g00 | g01 | g02 | ... | gFF |       512 bytes
   |     \_______________________
   |                             \
   | m00 | m01 | m02 | ... | mFF |       256 bytes
   |     \_______________________ 
   |                             \
   | k00 | k01 | k02 | ... | kFF |       32 bytes (256 bits)

Note that there is no direct way to distinguish a full table from an empty one.
This is easily allievated by checking whether any bit in the bitmap is 1 or 0.


Cache
~~~~~

To improve the performance of frequent allocations, pages are buffered inside a
stack. There are 256 colored stacks to improve the performance of architectures
using physically tagged caches.

When a stack is empty on allocation, _all_ stacks are refilled with a single
entry. If a stack is already full during the refill, the page will remain in
the backing store.

When a stack is full on deallocation, _all_ stacks have one entry removed. If a
stack is already empty, nothing happens.

This should allow near-optimal performance for the common cases.


Process staggering
~~~~~~~~~~~~~~~~~~

To optimize cache usage by multiple concurrent processes the PID is added as an
offset to the color.
