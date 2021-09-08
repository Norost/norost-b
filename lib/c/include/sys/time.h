#ifndef __LIBC_SYS_TIME_H
#define __LIBC_SYS_TIME_H

#include <stdint.h>

typedef uint64_t time_t;
typedef uint32_t suseconds_t;

struct timeval {
	time_t tv_sec;
	suseconds_t tv_usec;
};

struct timespec {
	time_t tv_sec;
	long tv_nsec;
};

#endif
