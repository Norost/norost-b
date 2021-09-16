===========================
Inter-process communication
===========================

IPC is achieved by "process hopping": a thread stores data to be transmitted in
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


Ports
~~~~~

Processes are not addressed directly, instead a process estabilishes a port
through which communications occur.

There are three types of ports:


Named ports
'''''''''''

Named ports are globally accessible and are addressed by a string.


Anonymous ports
'''''''''''''''

Anonymous ports can be created when spawning a process or when returning from a
hop.


Callstack model
~~~~~~~~~~~~~~~

When a thread hops, it can choose whether a checkpoint should be
created. This checkpoint holds the stack pointer and program counter at the
moment of the call, allowing the state before the call to be restored.

The checkpoint is added to a list in the receiving process' structure. This
process can use the checkpoint at any time. On process destruction, all
checkpoints are iterated and the waiting processes are notified.


Notifications
~~~~~~~~~~~~~

There are a few special types of IPC performed by the kernel. These all have a
dedicated handler that can be registered by a process:

* Process exit
* Page fault
* Memory exhaustion
