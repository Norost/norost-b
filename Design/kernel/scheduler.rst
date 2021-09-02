=========
Scheduler
=========

The scheduler is responsible for ensuring every thread gets a chance to run. It
is able to pre-empt threads when an interrupt occurs.

To prevent a process group from using the majority of CPU time, each group has
a dynamic priority that increases the longer each thread in that group runs.


Executors
~~~~~~~~~

Each hart has a single executor capable of executing threads. All of them have:

* Some scratch space for privilege switches.
* A pointer to a stack.
* A reference to the current thread being executed.


Saving thread state
~~~~~~~~~~~~~~~~~~~

General-purpose registers are saved as soon as a thread switch occurs.

If any other registers were accessed they are saved too. Otherwise they are
left untouched.

Since many threads won't need any registers other than the general-purpose
ones, storage for other registers are allocated as needed.


Implementation
~~~~~~~~~~~~~~

Threads are put in a per-group queue. A queue is a circular linked list of
threads. Each queue is put in a priority queue which is sorted based on
dynamic priority.

For simplicity, all queues use a lock as locks are usually simpler than
lock-free algorithms and it is unlikely each queue will be contented often.
