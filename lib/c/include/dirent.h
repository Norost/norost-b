#ifndef __LIBC_DIRENT_H
#define __LIBC_DIRENT_H

#include <sys/types.h>
#include <stdint.h>
#include <limits.h>

struct dirent {
	ino_t d_ino;
	char d_name[NAME_MAX];
};

int alphasort(const struct dirent **lhs, const struct dirent **rhs);

int closedir(DIR * dir);

int dirfd(DIR * dir);

DIR *fdopendir(int fd);

DIR *opendir(const char *path);

struct dirent *readdir(DIR * dir);

void rewinddir(DIR * dir);

int scandir(const char *, struct dirent **, int (*)(const struct dirent *),
	    int (*)(const struct dirent **, const struct dirent **));

void seekdir(DIR * dir, long loc);

long telldir(DIR * dir);

#endif
