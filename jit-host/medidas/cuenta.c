/*
    U7: plugin de QEMU que cuenta las instrucciones ARM ejecutadas, aparte las del código que traduce el JIT
    (la memoria del código traducido, que el banco ejecuta en JIT_RX = 0x3000000000, 64 MB) y todas las demás
    (el traductor, las funciones de C de la FPU, el banco). Al acabar escribe:

        jit <instrucciones en código traducido> total <todas>

    qemu-aarch64 -plugin ./cuenta.so -d plugin -D salida.txt build-le/jit-host programa.bin
*/
#include <inttypes.h>
#include <stdio.h>
#include <qemu-plugin.h>

QEMU_PLUGIN_EXPORT int qemu_plugin_version = QEMU_PLUGIN_VERSION;

#define JIT_RX  0x3000000000ULL
#define JIT_LEN (64ULL << 20)

static struct qemu_plugin_scoreboard *marcador;
static qemu_plugin_u64 jit, total;

struct cuentas { uint64_t jit, total; };

static void al_traducir(struct qemu_plugin_tb *tb, void *p)
{
    uint64_t pc = qemu_plugin_tb_vaddr(tb);
    size_t n = qemu_plugin_tb_n_insns(tb);
    qemu_plugin_register_vcpu_tb_exec_inline_per_vcpu(tb, QEMU_PLUGIN_INLINE_ADD_U64, total, n);
    if (pc >= JIT_RX && pc < JIT_RX + JIT_LEN)
        qemu_plugin_register_vcpu_tb_exec_inline_per_vcpu(tb, QEMU_PLUGIN_INLINE_ADD_U64, jit, n);
}

static void al_salir(void *p)
{
    char s[128];
    snprintf(s, sizeof s, "jit %" PRIu64 " total %" PRIu64 "\n", qemu_plugin_u64_sum(jit), qemu_plugin_u64_sum(total));
    qemu_plugin_outs(s);
    qemu_plugin_scoreboard_free(marcador);
}

QEMU_PLUGIN_EXPORT int qemu_plugin_install(qemu_plugin_id_t id, const qemu_info_t *info, int argc, char **argv)
{
    marcador = qemu_plugin_scoreboard_new(sizeof(struct cuentas));
    jit = qemu_plugin_scoreboard_u64_in_struct(marcador, struct cuentas, jit);
    total = qemu_plugin_scoreboard_u64_in_struct(marcador, struct cuentas, total);
    qemu_plugin_register_vcpu_tb_trans_cb(id, al_traducir, NULL);
    qemu_plugin_register_atexit_cb(id, al_salir, NULL);
    return 0;
}
