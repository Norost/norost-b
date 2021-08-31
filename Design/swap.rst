====
Swap
====

To reduce memory pressure, pages can be *swapped* out to disk. Unlike other
OSes, this is done by the processes themselves. This allows the process to
decide which memory is critical for operation and which isn't, e.g. executeable
memory is likely more important than other data.

Shared memory can't be directly swapped out. Instead, it is expected that a
process that doesn't immediately need it simply unmaps it. The owner can
optionally save it to disk and unmap the memory.
