=========
Allocator
=========

While a microkernel could feasibly be implemented without a dedicated global
allocator, having one makes development significantly easier. It also allows
moving to a hybrid kernel at a later stage easier.

The current algorithm is based on ``dlmalloc``.


Structure
~~~~~~~~~

There are two important structures: the regions on the heap itself and the list
of buckets.


Bins
''''

Free regions are kept track of in bins. Each bin covers a range of sizes.

Bin sizes:

===== =========
Count   Delta
===== =========
64            8
32           64
16          512
 8           4K    
 4          32K    
 2         256K    
 1    remainder
===== =========

Note that:

* The first 2 bins are unused as a region is at least 16 bytes large.
* The delta increases by a factor 8

::

   |   2 |   3 |   4 |     |  64 |     | 
   |  16 |  24 |  32 | ... | 512 |     | ... | 2^n |
   | --- | --- | --- | --- | --- | --- | --- | ----|


Region
''''''

::

   | ...       |
   | size      |
   |===========|
   | size (0)  |
   | user data |
   | ...       |
   | size (0)  |
   |===========|
   | size (n)  |
   | prev      |
   | next      |
   | ...       |
   | size (n)  |
   |===========|
   | size      |
   | ...       |


Allocation
~~~~~~~~~~

When allocating a region a lookup is first performed for a fitting entry in
the appriopiate bucket. If the bucket is empty, other buckets are checked.
If all buckets are empty, new heap memory is mapped in.

When a region is allocated, the first and last ``size`` integers are set to 0
and a pointer to the start of the region in the middle is returned.

A ``size`` is 4 bytes long and indicates the size of the subregion in bytes.


Freeing
~~~~~~~

When freeing a block of memory, the bits right before and after the total
region (including the zeroed bits) are checked. If they are *not* zero, it
means those blocks are free and they will be coalesced into one large block.

Finally, the first and last ``size`` integers of the coalesced region are set
to represent the total size of the region and a pointer to the start of the region
is added to the appropriate bucket.

Since the bucket is a linked list, a single offset at the start of the region
is set to the next region. This offset 4 bytes large and is relative to the
start of the heap. A pointer to the previous region is also added so removal
from a bucket has constant (i.e. ``O(1)``) overhead.
