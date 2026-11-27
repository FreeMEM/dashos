/*
    DashOS, X7: el cerrojo que usa tlsf.c de Emu68, para x86-64 (el de Emu68 espera con instrucciones de ARM). Fichero
    propio de DashOS (MPL-2.0).
*/
#ifndef _SPINLOCK_H
#define _SPINLOCK_H

#include <stdbool.h>

typedef struct {
    volatile char lock;
} __attribute__((aligned(64))) spinlock_t;

static inline void spinlock_init(spinlock_t *s) { s->lock = 0; }

static inline void spinlock_acquire(spinlock_t *s)
{
    while (__atomic_test_and_set(&s->lock, __ATOMIC_ACQUIRE))
        __builtin_ia32_pause();
}

static inline void spinlock_release(spinlock_t *s) { __atomic_clear(&s->lock, __ATOMIC_RELEASE); }

static inline bool spinlock_try_acquire(spinlock_t *s) { return !__atomic_test_and_set(&s->lock, __ATOMIC_ACQUIRE); }

#endif
