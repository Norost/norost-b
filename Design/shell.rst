==========
User shell
==========

The shell uses a LISP dialect for commands.

e.g.

::
   (pipe "stdout"
         (ls "-l" "-a")
         (wc "-l"))

To reduce typing, it is trivial to create *persistent* functions:

::
   (defun count-files
          (tags)
          (pipe "stdout"
                (ls "-l" "-a" tags)
                (wc "-l")))

These functions are saved as files and are tagged with ``exec-with:shell``.

To reduce clutter by excessive functions, functions can be enabled for specific
contextes only with ``context:name`` tags. Contexts are entered with
``enter-context``, ``leave-context`` or by starting a command with
``with-context``, e.g.

::
   (with-context "file-audit"
                 (count-files "bob"
                              (list "2019"
                                    "hello-world"
                                    "executable")))
