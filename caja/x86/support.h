/*
    DashOS, X7: lo que la caja de x86 usa de src/support.h de Emu68 (que lleva cabeceras de ARM): la salida con formato,
    las funciones de memoria y el montón de TLSF. Las da soporte.c. Fichero propio de DashOS (MPL-2.0).
*/
#ifndef DASHOS_CAJA_X86_SUPPORT_H
#define DASHOS_CAJA_X86_SUPPORT_H

#include <stdint.h>
#include <stddef.h>
#include <stdarg.h>

typedef void (*putc_func)(void *data, char c);
void vkprintf_pc(putc_func putc_f, void *putc_data, const char *format, va_list args);
void kprintf(const char *format, ...);

void *memcpy(void *d, const void *s, size_t n);
void *memmove(void *d, const void *s, size_t n);
void *memset(void *d, int c, size_t n);
void bzero(void *d, size_t n);

/* El montón general (TLSF), como en Emu68. */
extern void *tlsf;

#endif
