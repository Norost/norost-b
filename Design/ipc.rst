===========================
Inter-process communication
===========================

IPC is achieved by "thread hopping": a thread stores data to be transmitted in
some of its registers and asks the kernel to switch it to another process.

If the other process has a notification handler set up, it will set the
thread's program counter to that of the handler. If not, the call will fail.

The hopped thread will have no stack. The receiving process needs to allocate
a stack if necessary.


Asynchronous communication
~~~~~~~~~~~~~~~~~~~~~~~~~~

If a thread needs to keep running while making a request, it can create a new
thread with the target process and message contents already set.


Sharing memory
~~~~~~~~~~~~~~

To share a large amount of memory, mappings can be moved or shared. The sending
process specifies a range which the receiving process can then accept.

Move mappings will unmap the region in the sender's address space if the
receiver accepts it. Otherwise, it will remain in the sender's address space.

Share mappings will not unmap the region but will increase a reference counter
per page. Each page size (4k/2M/1G...) has a separate reference counter to
improve sharing speed.
