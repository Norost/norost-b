#ifndef __STD_SYS_UIO_H
#define __STD_SYS_UIO_H

#include <stddef.h>

struct iovec {
	void *iov_base;
	size_t iov_len;
};

ssize_t readv(int fd, const struct iovec *iov, int iovcnt);

ssize_t writev(int fd, const struct iovec *iov, int iovcnt);

#undef ssize_t

#endif
