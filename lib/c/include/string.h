#ifndef __LIBC_STRING_H
#define __LIBC_STRING_H

#include <stddef.h>

extern size_t strlen(const char *);

extern void *memcpy(void *dest, const void *src, size_t n);

extern void *memmove(void *dest, const void *src, size_t n);

extern void *memset(void *dest, int c, size_t n);

extern char *strcat(char *dest, const char *src);

extern char *strncat(char *dest, const char *src, size_t n);

extern char *strchr(const char *, int);

extern char *strtok(char *str, const char *delim);

extern int strcmp(const char *a, const char *b);

extern int strncmp(const char *a, const char *b, size_t n);

extern char *strcpy(char *dest, const char *src);

extern char *strncpy(char *dest, const char *src, size_t n);

#endif
