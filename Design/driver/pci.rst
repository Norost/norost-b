===
PCI
===

Since PCI is very common as an interface between devices and OSes, dedicated
support has been added in the kernel.

There are X syscalls related to managing PCI devices.


pci_map_any
~~~~~~~~~~~

Map any free PCI device with the given ID.

Parameters
''''''''''

* ``id``, ``u32``: the ID of the device. The upper 16 bits are the vendor ID,
  the lower 16 bits are the device ID in little-endian format.

* ``address``, ``*const Page``: the address where to map the configuration space
  to. It is mapped as read-only.

Returns
'''''''

* ``handle``, ``u32``: A handle representing the device.


pci_map_bar
~~~~~~~~~~~

Map the memory region pointed at by the given BAR. Calling this with an address
of ``null`` will not map the region but it will return the size of this region.

Parameters
''''''''''

* ``handle``, ``u32``: the device being referenced.

* ``bar``, ``u8``: the BAR the be mapped.

* ``address``, ``*const Page``: the page where to map the BAR.

Returns
'''''''

This function always returns the total size of the mapped region, even if
mapping failed.
