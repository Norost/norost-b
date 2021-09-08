#ifndef __LIBC_STDLIB_H
#define __LIBC_STDLIB_H

#include <stddef.h>

extern void *calloc(size_t, size_t);

extern void free(void *);

extern void *malloc(size_t);

extern int abs(int);

extern int atoi(const char *);

extern char *getenv(const char *);

void exit(int);

#endif
