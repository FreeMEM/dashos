/* U7: el sysroot de la toolchain big-endian solo trae gnu/stubs-lp64_be.h; al compilar con -mlittle-endian, las
   cabeceras de glibc piden este. Solo define macros __stub_*, que el JIT no usa. */
#include <gnu/stubs-lp64_be.h>
