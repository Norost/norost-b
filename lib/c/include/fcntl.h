#ifndef __LIBC_FCNTL_H
#define __LIBC_FCNTL_H

#include <stddef.h>

typedef signed long ssize_t;

ssize_t write(int fd, const void *buf, size_t count);

ssize_t read(int fd, void *buf, size_t nbyte);

#endif
