==========
Parameters
==========

Paramaters are not passed directly via "command line" arguments. Instead,
arguments are set in a memory region defined by an ELF section.

Parameters structure format::

   Parameter            Type        Default value   

   magic                u32         0x4e724400
   max_argument_count   u16
   argument_count       u16         0
   max_children_count   u16
   max_children         u16         0
   master (unused)      u32
   string_base          *const u8   0
   ...
   name                 u32
   value                u32
   name_len             u16
   value_len            u16
   ...

The structure MUST be located in a writeable segment!


Arguments
~~~~~~~~~

The parameters structure essentially represent a single node from a device tree
without the interpreter nonsense.

Each argument represents a single property of a node. The names and values are
put in a memory location defined by the program spawning the driver (usually
the top of the stack).

The name is a valid UTF-8 string[#]_, the value can be any raw value.

There are some "pseudo-properties" that are not valid device tree properties
but may be specified by the process spawning the driver.

.. [#] Software should not implicitly assume it is correct! Always check before
   using the value.


Pseudo-properties
'''''''''''''''''

==================== =================================================
      Property                          Description
==================== =================================================
``.address-cells``   ``#address-cells`` specified by the parent node
``.size-cells``      ``#size-cells`` specified by the parent node
``.interrupt-cells`` ``#interrupt-cells`` specified by the parent node
==================== =================================================


Children
~~~~~~~~

This value is reserved in case a need for passing child nodes to drivers
becomes necessary.


Getting other nodes
~~~~~~~~~~~~~~~~~~~

It may be necessary to get info about other nodes. To do so a request can be
sent to the "master" process. The master process only accepts ``phandles``. If
the node is found, a buffer or a page with the node data is returned. This data
is in the same format of that of the parameters structure minus the
``master_address`` and ``string_base`` properties::

   Parameter            Type        Default value   

   magic                u32         0x4e724480
   max_argument_count   u16
   argument_count       u16         0
   max_children_count   u16
   max_children         u16         0
   ...
