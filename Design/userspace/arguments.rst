=========
Arguments
=========

Every program needs to be able to receive some form of arguments, whether it's
explicit like command line arguments or implicit like where to pipe stdout to.


.norost sections
~~~~~~~~~~~~~~~~

The primary method for passing arguments is via the .norost sections present in
ELF files.


.norost.args
''''''''''''

This section covers a structure with the following format::

   struct norost_args {
       magic: u32
       cmd_args_count: u16
       _padding: u16
       strings_base: *const small_str
       data_in: file
       data_out: file
       data_err: file
       cmd_args: u32
   }

   struct file {
       process: u32
       object: u32
   }

   struct small_str {
      len: u16
      chars: [u8]
   }


.norost.driver
''''''''''''''

See the Driver_ document for details

.. Driver ../driver/Index.rst
