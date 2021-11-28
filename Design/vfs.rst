===================
Virtual File System
===================

Classic filesystem:

- Files organized in directories.
- Files have one owner & user/group/system permissions

Problems:

- Some files belong in multiple categories (can be addressed with symlinks but
  is clumsy, or overly deep hierarchies), e.g. "photos/2019/beach_a.jpg" and/or
  "photos/beach/beach_a.jpg.
- Multi-owner files are difficult.

Solution: pure tag-based VFS.

- Search for files based on tag matches, e.g. "photo" + "beach" + "2019"
- File owners can be specified by tags, e.g. "owner:bob" + "owner:alice"
- Additional properties can be specified, including exotic ones, e.g.
  "executable", "block-dev", "exec-with:python3", ...


Implementation
~~~~~~~~~~~~~~

Files are identified with integer IDs, tags refer to these IDs.

File list is a big list.

Tags are tracked inside a map. Each tag has one map with file IDs.

Each file has a name. Names do not need to be unique. To open a file with a
conflicting name and tags, use the file ID.

A map of file names to file IDs exist to speed up name lookups to files.

A separate mapping exists for each file volume. This is to avoid unexpected
latencies, conflicts & failures as well as simplify the implementation.


Example diagram
~~~~~~~~~~~~~~~

::

   BOOT (root-only)
   | 0:0 kernel
   | 0:1 grub
   | 0:2 grub.cfg

   HOME (user-owner)
   | 1:0 linux.iso.mkv      [owner:bob]      [movie]
   | 1:1 linux.iso.mkv      [owner:bob]      [movie]
   | 1:2 linux.iso.mkv      [owner:alice]    [movie]
   | 1:6 rsync-linux-isos   [owner:bob]      [executable] [exec-with:shell]
   | 1:8 cats.mp4           [owner:alice]    [owner:bob]  [memes]
   | 1:9 rsync              [allow-read-all] [executable]

   NAS-0 (global)
   | 2:2 more_cats.webm     [animals] [cats]
   | 2:3 dogs.mov           [animals] [dogs]
   | 2:6 doggie.flv         [animals] [dogs]
   | 2:9 woof.gif           [animals] [dogs]
