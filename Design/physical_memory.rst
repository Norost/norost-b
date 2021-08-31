===============
Physical memory
===============

Physical memory is managed by the kernel. All memory is tracked using a bitmap.
Freed pages are put in colored stacks to improve the performance of frequent
allocations and deallocations.
