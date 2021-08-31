=============
Shared memory
=============

To reduce copying, reduce memory pressure and improve cache efficiency pages
can be shared.

Reference counting is used to determine when a page needs to be freed.

Sets
~~~~

To share pages, they must be put in a set first. A set has a counter indicating
the total amount of references to each page. When sharing a set with another
process, the counter is increased by one.

Pages can be added to a set at any time but no pages can be removed.

Shared sets are kept track of in a per-process table along with a counter
indicating how many pages are used by said process. When this counter reaches
zero, the set is removed from the process and the set counter is reduced by
one.

When the set counter reaches 0 all pages inside are returned to the physical
memory manager.


Security
''''''''

To prevent memory exhaustion, each set has an owner. The owner is the process
which created the set originally. Only the owner can add pages to the set.


Implementation
''''''''''''''


