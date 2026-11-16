//! X7a: RelevoJIT (el intérprete y el JIT del 68k para x86-64, `~/Projects/RelevoJIT`, MPL-2.0) dentro de un proceso de
//! DashOS, sin caja Amiga. Enlaza el crate `relevo` sin std (característica "relevo") y le da la memoria de código del
//! JIT con objetos de memoria de DashOS: el mismo objeto proyectado dos veces, para escribir (`memoria_proyectar`) y
//! para ejecutar (`memoria_proyectar_codigo`, permiso 4), sin que ninguna página se pueda escribir y ejecutar a la vez.
//!
//! Lo que hace, en este orden:
//!
//! 1. Los programas de `jit-host/pruebas68k` con el intérprete y con el JIT, como `tests/runbin.c` de RelevoJIT en
//!    Linux: la misma máquina (2 MB de RAM, el programa en $140000, la pila en $1FEFFC con 0 como dirección de vuelta),
//!    una CPU por motor para todos los programas, y la misma línea por programa y motor. Cada programa se copia en la
//!    memoria del 68k a mano, así que tras copiarlo se avisa con `relevo_invalidate` (si no, el JIT seguiría con el
//!    código traducido del anterior). Las líneas se comparan con las de Linux (`jit-host/pruebas68k/ref/relevo.txt`).
//! 2. Un banco: los programas `arith`, `copy` y `bytes` de `tests/bench.c` de RelevoJIT, en un 68020, con los dos
//!    motores; los registros del final tienen que coincidir, y dice los MIPS de cada uno.
//!
//! Sale con 70 si todo cuadra; con 1 si algo no, y dice qué.
//!
//! Solo en x86-64: en ARM el JIT de RelevoJIT no existe (allí está Emu68).

#![no_std]
#![no_main]

extern crate alloc;

dashos::programa!(principal);
dashos::monton!();

#[cfg(not(target_arch = "x86_64"))]
fn principal() -> u64 {
    dashos::println!("[relevo] solo en x86-64");
    0
}

#[cfg(target_arch = "x86_64")]
fn principal() -> u64 {
    x86::principal()
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use core::ffi::{c_int, c_uint, c_void};
    use core::ptr::null_mut;

    use dashos::println;
    use relevo::capi::*;

    // Los valores de `include/relevo.h` de RelevoJIT (relevo_model, relevo_fpu, relevo_engine, relevo_status).
    const M68020: c_uint = 2;
    const M68040: c_uint = 4;
    const FPU_68882: c_uint = 2;
    const FPU_INTERNA: c_uint = 3;
    const INTERPRETE: c_uint = 0;
    const JIT: c_uint = 1;
    const SALIR_EN_PC_CERO: c_uint = 1;
    const PRESUPUESTO: c_uint = 0;
    const PC_CERO: c_uint = 1;
    const PARADA: c_uint = 2;
    const PARADA_ANFITRION: c_uint = 3;
    const DETENIDA: c_uint = 4;

    // ---- La memoria de código del JIT ----------------------------------------------------------------------------

    /// Dónde se proyecta la memoria de código: en lo libre desde 384 GB (`docs/referencia/kernel.md`), lejos del
    /// montón (64 GB) y de sus pedidos grandes (480 GB). Una sola CPU con JIT viva a la vez.
    const ESCRIBIR_EN: usize = 0x60_0000_0000;
    const EJECUTAR_EN: usize = 0x61_0000_0000;
    /// 8 MB: el kernel da objetos de hasta 16 MB. Si se llena, RelevoJIT tira lo traducido y empieza de nuevo.
    const TAM_CODIGO: usize = 8 << 20;

    /// Lo que el JIT pidió a la memoria de código: la llave del objeto y cuántas veces escribió.
    #[derive(Default)]
    struct Codigo {
        llave: u32,
        escrituras: u64,
        bytes: u64,
    }

    /// `relevo_code_memory.map`: un objeto de memoria de `tam` bytes, proyectado para escribir en `ESCRIBIR_EN` y para
    /// leer y ejecutar en `EJECUTAR_EN`.
    unsafe extern "C" fn proyectar(ctx: *mut c_void, tam: usize, rw: *mut *mut c_void, rx: *mut *mut c_void) -> c_int {
        // SAFETY: `ctx` es el `Codigo` que `cpu_nueva` puso en la tabla, vivo mientras la CPU, y RelevoJIT no llama a
        // la vez a otra función de la tabla.
        let c = unsafe { &mut *ctx.cast::<Codigo>() };
        if c.llave != 0 {
            return -1;
        }
        let m = dashos::memoria_crear(tam);
        if (m as i32) <= 0 {
            return -1;
        }
        if dashos::memoria_proyectar(m, ESCRIBIR_EN, true) != 0 {
            dashos::cerrar(m);
            return -1;
        }
        if dashos::memoria_proyectar_codigo(m, EJECUTAR_EN) != 0 {
            dashos::memoria_quitar(ESCRIBIR_EN);
            dashos::cerrar(m);
            return -1;
        }
        c.llave = m;
        // SAFETY: `rw` y `rx` son de RelevoJIT, para esto (relevo.h).
        unsafe {
            *rw = ESCRIBIR_EN as *mut c_void;
            *rx = EJECUTAR_EN as *mut c_void;
        }
        0
    }

    /// `relevo_code_memory.unmap`: al destruir la CPU.
    unsafe extern "C" fn quitar(ctx: *mut c_void, _rw: *mut c_void, _rx: *mut c_void, _tam: usize) {
        // SAFETY: como en `proyectar`.
        let c = unsafe { &mut *ctx.cast::<Codigo>() };
        dashos::memoria_quitar(ESCRIBIR_EN);
        dashos::memoria_quitar(EJECUTAR_EN);
        dashos::cerrar(c.llave);
        c.llave = 0;
    }

    /// `relevo_code_memory.end_write`: el JIT escribió `tam` bytes desde `desde`. En x86 no hay caché de instrucciones
    /// que limpiar, pero el código se escribió por otra dirección lineal que la de ejecutarlo: antes de ejecutarlo, una
    /// instrucción que serializa, CPUID (Intel SDM, volumen 3, "Self-Modifying Code"; como el paso 37 de `malo`).
    unsafe extern "C" fn escrito(ctx: *mut c_void, _desde: usize, tam: usize) {
        // SAFETY: como en `proyectar`.
        let c = unsafe { &mut *ctx.cast::<Codigo>() };
        c.escrituras += 1;
        c.bytes += tam as u64;
        // SAFETY: CPUID existe en todo x86-64 y solo lee.
        #[allow(unused_unsafe)]
        let _ = unsafe { core::arch::x86_64::__cpuid(0) };
    }

    // ---- Una CPU -----------------------------------------------------------------------------------------------------

    struct Cpu {
        cpu: *mut relevo_cpu,
        mem: Vec<u8>,
        /// La tabla y su contexto: en el montón, para que no se muevan mientras viva la CPU.
        _tabla: alloc::boxed::Box<relevo_code_memory>,
        codigo: alloc::boxed::Box<Codigo>,
    }

    impl Cpu {
        fn nueva(modelo: c_uint, fpu: c_uint, motor: c_uint, tam: usize) -> Option<Cpu> {
            let mut mem = vec![0u8; tam];
            let mut codigo = alloc::boxed::Box::new(Codigo::default());
            let tabla = alloc::boxed::Box::new(relevo_code_memory {
                ctx: (&raw mut *codigo).cast(),
                size: TAM_CODIGO,
                map: Some(proyectar),
                unmap: Some(quitar),
                begin_write: None,
                end_write: Some(escrito),
            });
            let cfg = relevo_config {
                model: modelo,
                fpu,
                engine: motor,
                memory: mem.as_mut_ptr(),
                memory_size: tam,
                address_mask: 0,
                options: SALIR_EN_PC_CERO,
                host: relevo_host { ctx: null_mut(), read: None, write: None, line_a: None, line_f: None, reset: None },
                code_memory: &*tabla,
            };
            // SAFETY: la configuración es válida; la memoria del 68k (`mem`, que no se mueve: es un Vec que no crece)
            // y la tabla con su contexto viven en `Cpu` hasta `relevo_destroy`, en `drop`.
            let cpu = unsafe { relevo_create(&cfg) };
            (!cpu.is_null()).then_some(Cpu { cpu, mem, _tabla: tabla, codigo })
        }

        /// Los registros vivos. Cada llamada hace una referencia nueva: no guardar dos.
        #[allow(clippy::mut_from_ref)]
        fn regs(&self) -> &mut relevo_regs {
            // SAFETY: la CPU es de este hilo y no corre ahora.
            unsafe { &mut *relevo_get_regs(self.cpu) }
        }

        fn instrucciones(&self) -> u64 {
            // SAFETY: la CPU es de este hilo.
            unsafe { relevo_instructions(self.cpu) }
        }

        fn correr(&mut self, presupuesto: u64) -> c_uint {
            // SAFETY: la CPU es de este hilo; no hay funciones del anfitrión.
            unsafe { relevo_run(self.cpu, presupuesto) }
        }

        /// El cargador cambió la memoria del 68k a mano: que el JIT tire lo traducido de ahí.
        fn invalidar(&mut self, desde: u32, tam: u32) {
            // SAFETY: la CPU es de este hilo.
            unsafe { relevo_invalidate(self.cpu, desde, tam) }
        }

        fn reiniciar(&mut self) {
            // SAFETY: la CPU es de este hilo.
            unsafe { relevo_reset(self.cpu) }
        }

        fn estadisticas(&self) -> jit_stats {
            let mut s = jit_stats::default();
            // SAFETY: la CPU es de este hilo; `s` es del tamaño que espera.
            unsafe { jit_get_stats(self.cpu, &mut s) };
            s
        }
    }

    impl Drop for Cpu {
        fn drop(&mut self) {
            // SAFETY: hecha por `relevo_create` y no destruida.
            unsafe { relevo_destroy(self.cpu) };
        }
    }

    // ---- Los programas de jit-host/pruebas68k ----------------------------------------------------------------------

    const MEM: usize = 0x20_0000;
    const CARGAR_EN: usize = 0x14_0000;
    const PILA: u32 = 0x1F_EFFC;
    const MAX_INSTRUCCIONES: u64 = 100_000_000;

    macro_rules! prueba68k {
        ($n:literal) => {
            ($n, include_bytes!(concat!("../../../jit-host/pruebas68k/", $n, ".bin")) as &[u8])
        };
    }

    /// En el orden de la referencia: el estado de la FPU pasa de un programa al siguiente, como en `runbin`.
    const PROGRAMAS: [(&str, &[u8]); 7] =
        [prueba68k!("c1"), prueba68k!("c1-o0"), prueba68k!("t1"), prueba68k!("t2"), prueba68k!("t3"), prueba68k!("t4"), prueba68k!("t5")];

    /// Lo que dio `build/runbin` de RelevoJIT en Linux con los mismos programas y en el mismo orden.
    const REFERENCIA: &str = include_str!("../../../jit-host/pruebas68k/ref/relevo.txt");

    fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }

    const FNV_INICIO: u64 = 0xcbf2_9ce4_8422_2325;

    /// Como `regs_hash` de `runbin`: D0-D7, A0-A7, PC, SR, FP0-FP7 (signo y exponente, mantisa), FPCR, FPSR y FPIAR, cada
    /// uno en big-endian.
    fn suma_registros(r: &relevo_regs) -> u64 {
        let mut h = FNV_INICIO;
        for v in r.d.iter().chain(r.a.iter()).chain([r.pc].iter()) {
            h = fnv(h, &v.to_be_bytes());
        }
        h = fnv(h, &r.sr.to_be_bytes());
        for f in r.fp.iter() {
            h = fnv(h, &f.sign_exponent.to_be_bytes());
            h = fnv(h, &f.mantissa.to_be_bytes());
        }
        for v in [r.fpcr, r.fpsr, r.fpiar] {
            h = fnv(h, &v.to_be_bytes());
        }
        h
    }

    fn nombre_fin(st: c_uint) -> &'static str {
        match st {
            PC_CERO => "pc0",
            PARADA => "stop",
            PARADA_ANFITRION => "host",
            DETENIDA => "halted",
            _ => "budget",
        }
    }

    /// Un programa en una CPU: la línea de `runbin`.
    fn ejecutar(c: &mut Cpu, motor: &str, nombre: &str, binario: &[u8]) -> String {
        c.mem.fill(0);
        c.mem[..4].copy_from_slice(&PILA.to_be_bytes());
        c.mem[4..8].copy_from_slice(&(CARGAR_EN as u32).to_be_bytes());
        c.mem[CARGAR_EN..CARGAR_EN + binario.len()].copy_from_slice(binario);
        c.invalidar(0, MEM as u32);
        {
            let r = c.regs();
            r.d = [0; 8];
            r.a = [0; 8];
            r.usp = 0;
            r.msp = 0;
        }
        c.reiniciar();
        let inicio = c.instrucciones();
        let mut st;
        loop {
            st = c.correr(1_000_000);
            if st != PRESUPUESTO || c.instrucciones() - inicio >= MAX_INSTRUCCIONES {
                break;
            }
        }
        format!(
            "{} {}: {} insns={} regs={:016x} mem={:016x}",
            nombre,
            motor,
            nombre_fin(st),
            c.instrucciones() - inicio,
            suma_registros(c.regs()),
            fnv(FNV_INICIO, &c.mem[0x10_0000..0x20_0000])
        )
    }

    /// El paso 1. Devuelve si todo es igual que en Linux.
    fn pruebas68k() -> bool {
        let interprete = Cpu::nueva(M68040, FPU_INTERNA, INTERPRETE, MEM);
        let jit = Cpu::nueva(M68040, FPU_INTERNA, JIT, MEM);
        let (Some(mut interprete), Some(mut jit)) = (interprete, jit) else {
            println!("  [relevo] no se pudo crear la CPU (¿la memoria de código?)");
            return false;
        };
        let referencia: Vec<&str> = REFERENCIA.lines().filter(|l| !l.is_empty()).collect();
        let mut lineas = Vec::new();
        let t0 = dashos::ahora_us();
        for (nombre, binario) in PROGRAMAS {
            lineas.push(ejecutar(&mut interprete, "interp", nombre, binario));
            lineas.push(ejecutar(&mut jit, "jit", nombre, binario));
        }
        let us = dashos::ahora_us() - t0;
        let mut distintas = 0;
        for (i, l) in lineas.iter().enumerate() {
            let igual = referencia.get(i) == Some(&l.as_str());
            if !igual {
                distintas += 1;
                println!("  [relevo] distinta de Linux: {}", l);
                println!("  [relevo]     en Linux era: {}", referencia.get(i).copied().unwrap_or("(nada)"));
            }
        }
        let s = jit.estadisticas();
        println!(
            "  [relevo] JIT: {} bloques traducidos, {} entradas, {} instrucciones en el intérprete, {} vaciados",
            s.blocks, s.runs, s.interp, s.flushes
        );
        println!(
            "  [relevo] memoria de código: {} escrituras avisadas, {} KB escritos; {} ms los 14",
            jit.codigo.escrituras,
            jit.codigo.bytes / 1024,
            us / 1000
        );
        let bien = distintas == 0 && lineas.len() == referencia.len() && s.blocks > 0 && jit.codigo.escrituras > 0;
        if bien {
            println!("  [relevo] {} programas de jit-host/pruebas68k con el intérprete y con el JIT, igual que en Linux", PROGRAMAS.len());
        } else {
            println!("  [relevo] {} líneas distintas de las {} de Linux", distintas, referencia.len());
        }
        bien
    }

    // ---- El banco -------------------------------------------------------------------------------------------------

    // Los programas de `tests/bench.c` de RelevoJIT (allí está su ensamblador), en $1000 con la pila en $8000.
    const ARITH: &[u16] = &[0x7200, 0x203C, 0x002D, 0xC6C0, 0xD280, 0xB382, 0x5380, 0x66F8, 0x4E75];
    const COPY: &[u16] = &[
        0x2E3C, 0x0000, 0x07D0, 0x41F9, 0x0002, 0x0000, 0x43F9, 0x0003, 0x0000, 0x303C, 0x03FF, 0x22D8, 0x51C8, 0xFFFC,
        0x5387, 0x66E6, 0x4E75,
    ];
    const BYTES: &[u16] = &[
        0x2E3C, 0x0000, 0x1770, 0x41F9, 0x0002, 0x0000, 0x3C3C, 0x00FF, 0x7000, 0x1218, 0xB23C, 0x0080, 0x6504, 0xD041,
        0x6002, 0x9041, 0x610A, 0x51CE, 0xFFEE, 0x5387, 0x66DC, 0x4E75, 0xB142, 0xC47C, 0x7FFF, 0x8640, 0x4A43, 0x6702,
        0x5244, 0x4E75,
    ];

    /// Un programa del banco en un motor: (instrucciones, µs, registros).
    fn banco_uno(palabras: &[u16], motor: c_uint) -> Option<(u64, u64, u64)> {
        let mut c = Cpu::nueva(M68020, FPU_68882, motor, 0x10_0000)?;
        for (i, w) in palabras.iter().enumerate() {
            c.mem[0x1000 + 2 * i..0x1002 + 2 * i].copy_from_slice(&w.to_be_bytes());
        }
        let mut x: u32 = 12345;
        for a in 0x20000..0x21000 {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            c.mem[a] = (x >> 16) as u8;
        }
        c.invalidar(0, 0x10_0000);
        {
            let r = c.regs();
            r.a[7] = 0x8000;
            r.pc = 0x1000;
        }
        let t0 = dashos::ahora_us();
        let st = c.correr(u64::MAX);
        let us = dashos::ahora_us() - t0;
        (st == PC_CERO).then(|| (c.instrucciones(), us, suma_registros(c.regs())))
    }

    /// El paso 2. Devuelve si los dos motores acaban igual en cada programa.
    fn banco() -> bool {
        let mut bien = true;
        for (nombre, palabras) in [("arith", ARITH), ("copy", COPY), ("bytes", BYTES)] {
            let (Some(i), Some(j)) = (banco_uno(palabras, INTERPRETE), banco_uno(palabras, JIT)) else {
                println!("  [relevo] banco {}: no acabó", nombre);
                bien = false;
                continue;
            };
            let mips = |(n, us, _): (u64, u64, u64)| n / us.max(1);
            let igual = i.0 == j.0 && i.2 == j.2;
            println!(
                "  [relevo] banco {}: {} instrucciones; intérprete {} MIPS, JIT {} MIPS{}",
                nombre,
                i.0,
                mips(i),
                mips(j),
                if igual { "" } else { " (MAL: acaban distinto)" }
            );
            bien &= igual;
        }
        bien
    }

    pub fn principal() -> u64 {
        let a = pruebas68k();
        let b = banco();
        if a && b { 70 } else { 1 }
    }
}
