================
Standard Library
================

The OS provides a standard library for common functionality. The reference
implementation is written in Rust.


Memory management
~~~~~~~~~~~~~~~~~

The library provides a structure that keeps track of reserved memory regions as
well as convienence functions to map pages in & out.


IPC table
~~~~~~~~~

The library provides a structure that emulates POSIX file descriptors. It
maintains a list of object descriptors.
