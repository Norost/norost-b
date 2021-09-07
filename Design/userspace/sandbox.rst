=======
Sandbox
=======

To improve isolation, processes can be sandboxed. When a process is sandboxed
_all_ system calls get forwarded to another process. This allows running
applications designed for other operating systems to run "natively".


Manipulating address space
~~~~~~~~~~~~~~~~~~~~~~~~~~

Since all system calls are intercepted, the sandboxed process is unable to
manage its own address space directly. Instead, the host process has to do this
in it's stead.
