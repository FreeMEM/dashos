/* Banco del sistema de archivos en Linux (paso F de docs/MULTITAREA_LOOP.md): ext4 en un disco NVMe de QEMU virt, con
 * el kernel genérico de Debian para arm64. Va como /init de un initramfs (tools/medir_linux/hacer_fs.sh) que lleva
 * los módulos de NVMe y ext4. Las mismas operaciones que usuario/src/bin/medir_fs.rs en DashOS. Cada línea
 * "[medir] fs op=... valor=... unidad=..." la lee tools/medir.py.
 *
 * DashOS guarda en el disco cada orden que cambia algo antes de contestar: para comparar con eso, las pruebas de
 * escribir van en dos sabores, "sync" (fsync tras cada escritura o cada fichero) y sin él (un fsync al final). */
#define _GNU_SOURCE
#include <dirent.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/reboot.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <unistd.h>

static uint64_t cuentas(void) {
    uint64_t t;
    __asm__ volatile("isb; mrs %0, cntvct_el0" : "=r"(t));
    return t;
}
static uint64_t frecuencia(void) {
    uint64_t f;
    __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(f));
    return f ? f : 1;
}
static double segundos(uint64_t c) { return (double)c / frecuencia(); }

static void cargar(const char *ko) {
    int fd = open(ko, O_RDONLY);
    if (fd < 0 || syscall(SYS_finit_module, fd, "", 4 /* MODULE_INIT_COMPRESSED_FILE */) != 0)
        printf("  no se carga %s\n", ko);
    if (fd >= 0) close(fd);
}

static void vaciar_cache(void) {
    sync();
    int fd = open("/proc/sys/vm/drop_caches", O_WRONLY);
    if (fd >= 0) { write(fd, "3", 1); close(fd); }
}

static void dato(const char *op, double valor, const char *unidad) {
    printf("[medir] fs op=%s valor=%.1f unidad=%s\n", op, valor, unidad);
}

#define GRANDE (8 << 20)
#define TROZO (64 << 10)
#define PEQUENOS 500
static char buf[TROZO];

static void escribir_grande(const char *ruta, int cada_vez) {
    uint64_t t0 = cuentas();
    int fd = open(ruta, O_CREAT | O_TRUNC | O_WRONLY, 0644);
    for (int i = 0; i < GRANDE / TROZO; i++) {
        write(fd, buf, TROZO);
        if (cada_vez) fdatasync(fd);
    }
    fsync(fd);
    close(fd);
    dato(cada_vez ? "escribir_8mb_sync" : "escribir_8mb", GRANDE / 1048576.0 / segundos(cuentas() - t0), "MB/s");
}

static void leer_grande(const char *ruta, const char *op) {
    uint64_t t0 = cuentas();
    int fd = open(ruta, O_RDONLY);
    while (read(fd, buf, TROZO) > 0) {}
    close(fd);
    dato(op, GRANDE / 1048576.0 / segundos(cuentas() - t0), "MB/s");
}

static void pequenos(int con_sync) {
    char r[64];
    mkdir("/mnt/peq", 0755);
    uint64_t t0 = cuentas();
    for (int i = 0; i < PEQUENOS; i++) {
        snprintf(r, sizeof r, "/mnt/peq/f%04d", i);
        int fd = open(r, O_CREAT | O_TRUNC | O_WRONLY, 0644);
        write(fd, buf, 4096);
        if (con_sync) fsync(fd);
        close(fd);
    }
    if (!con_sync) sync();
    dato(con_sync ? "crear_4k_sync" : "crear_4k", PEQUENOS / segundos(cuentas() - t0), "ficheros/s");
    if (con_sync) return;
    t0 = cuentas();
    for (int i = 0; i < PEQUENOS; i++) {
        snprintf(r, sizeof r, "/mnt/peq/f%04d", i);
        int fd = open(r, O_RDONLY);
        read(fd, buf, 4096);
        close(fd);
    }
    dato("leer_4k", PEQUENOS / segundos(cuentas() - t0), "ficheros/s");
    struct stat st;
    t0 = cuentas();
    for (int i = 0; i < PEQUENOS; i++) {
        snprintf(r, sizeof r, "/mnt/peq/f%04d", i);
        stat(r, &st);
    }
    dato("datos", PEQUENOS / segundos(cuentas() - t0), "ops/s");
    t0 = cuentas();
    DIR *d = opendir("/mnt/peq");
    int n = 0;
    while (readdir(d)) n++;
    closedir(d);
    dato("listar_500", 1.0 / segundos(cuentas() - t0), "listados/s");
}

static void borrar(int con_sync) {
    char r[64];
    uint64_t t0 = cuentas();
    for (int i = 0; i < PEQUENOS; i++) {
        snprintf(r, sizeof r, "/mnt/peq/f%04d", i);
        unlink(r);
        if (con_sync) { int fd = open("/mnt/peq", O_RDONLY); fsync(fd); close(fd); }
    }
    sync();
    dato(con_sync ? "borrar_sync" : "borrar", PEQUENOS / segundos(cuentas() - t0), "ficheros/s");
}

int main(void) {
    mount("proc", "/proc", "proc", 0, 0);
    mount("devtmpfs", "/dev", "devtmpfs", 0, 0);
    setvbuf(stdout, 0, _IOLBF, 0);
    printf("[medir] empieza\n");
    const char *mods[] = {"/m/nvme-auth.ko.xz", "/m/nvme-core.ko.xz", "/m/nvme.ko.xz", "/m/crc16.ko.xz",
                          "/m/mbcache.ko.xz", "/m/jbd2.ko.xz", "/m/crc32c_generic.ko.xz", "/m/libcrc32c.ko.xz",
                          "/m/ext4.ko.xz"};
    for (unsigned i = 0; i < sizeof mods / sizeof *mods; i++) cargar(mods[i]);
    for (int i = 0; i < 50 && access("/dev/nvme0n1", F_OK); i++) usleep(100000);
    if (mount("/dev/nvme0n1", "/mnt", "ext4", 0, 0)) {
        printf("  no se monta /dev/nvme0n1\n[medir] fin\n");
        reboot(RB_POWER_OFF);
    }
    /* Bytes al azar, como medir_fs.rs: no se comprimen (RedoxFS comprime con lz4 lo que se repite; ext4, nada). */
    uint32_t x = 12345;
    for (int i = 0; i < TROZO; i++) { x = x * 1664525u + 1013904223u; buf[i] = (char)(x >> 24); }
    escribir_grande("/mnt/grande", 0);
    escribir_grande("/mnt/grande_sync", 1);
    vaciar_cache();
    leer_grande("/mnt/grande", "leer_8mb_frio");
    leer_grande("/mnt/grande", "leer_8mb");
    pequenos(0);
    borrar(0);
    pequenos(1);
    borrar(1);
    umount("/mnt");
    printf("[medir] fin\n");
    reboot(RB_POWER_OFF);
    for (;;) pause();
}
