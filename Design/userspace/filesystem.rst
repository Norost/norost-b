==========
Filesystem
==========

A basic filesystem supports the following operations:

* Creating objects.
* Destroying objects.
* Reading objects.
* Modifying (writing) objects.
* Map objects.
* Listing child objects.
* Flushing/syncing objects.


List
~~~~

A list of objects has the following format::

   struct list {
       count: u32
       objects: [small_str]
   }

Note that ``small_str`` has a variable length, hence lookup is O(n). This is
not expected to be a problem as directories are usually iterated sequentially
anyways.
