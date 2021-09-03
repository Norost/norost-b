==============
Kernel drivers
==============

To improve performance, drivers can be implemented directly in the kernel.
These drivers are managed using the ``sys_driver`` call::

   sys_driver(
      name: *const u8,
      name_length: usize,
      arguments: *const u8,
   ) -> {
      status: usize,
      value: usize,
   }

To detect whether a driver exist, call ``sys_driver`` with ``arguments`` set
to ``null``. This will not initialize the driver. If a driver was found,
``status`` will be 0.

To initialize the driver, ``arguments`` must point to a parameters structure::

   Parameter            Type        Default value   

   magic                u32         0x4e724480
   max_argument_count   u16
   argument_count       u16         0
   max_children_count   u16
   max_children         u16         0
   master (unused)      u32
   strings_base         *const u8
   ...

A driver may be initialized multiple time for different devices. It is up to
the driver to handle this properly (this may include rejecting other devices).

Communication is done as with regular drivers except the address is -1, i.e.
the kernel's address.
