# Norost B Operating System

Norost B is an object-oriented OS built around a microkernel. It is mainly focused on
isolating processes from the rest of the system to improve security, portability &and;
scaling.

[Website][website]

[Design rationale][design]

## Features

- Supports x86-64
- Object-oriented interface
  - Files, network sockets ... are all objects.
  - Any process can create new objects.
  - IPC is performed via tables, which are also objects.
  - Processes can only perform operations on objects they have a handle to.
- Supported devices:
  - virtio-net
  - virtio-blk
- Supported filesystems:
  - FAT
- Networking (IP, TCP, UDP, DHCP, ICMP)
- Asynchronous I/O
- Rust standard library

## Building

[You will need a patched Rust compiler.][rust]

Once the compiler is properly configured, `mkiso.sh` will create a bootable image.
`run.sh` will run the OS in QEMU.

[design]: DESIGN.md
[rust]: thirdparty/rust
[website]: TODO
