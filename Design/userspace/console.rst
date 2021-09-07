=======
Console
=======

Many applications do not need a GUI. To simplify matters, a console process can
translate text to whatever output format (GUI, TUI, UART ...).

A console only needs to support read & write operations.


Automatically closing consoles
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

If a console wants to exit whenever an "attached" process exits, it should
watch said process.
