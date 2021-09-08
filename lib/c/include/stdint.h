#ifndef __LIBC_STDINT_H
#define __LIBC_STDINT_H

#ifdef __GNUC__
# include "stdint-gcc.h"
#else
# error "No stdint.h has been provided for this compiler"
#endif

#endif
