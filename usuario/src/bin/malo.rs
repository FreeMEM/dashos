//! C31: un programa hostil. Pasa al kernel punteros a memoria vacía, a su propio código (solo lectura) y fuera del
//! espacio de usuario, y comprueba que recibe errores en vez de tumbar el kernel; y que un mensaje que no se puede
//! entregar se queda en la cola, con sus llaves. Desde C32 también prueba los hallazgos de la auditoría del 03/11
//! (`docs/seguridad/`), un paso por cada uno. Sale con 31 si todo va bien, o con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{llamada, println};

dashos::programa!(principal);

const E_ARGUMENTO: i64 = -1;
const E_LLAVE: i64 = -2;
const E_TIPO: i64 = -6;
const E_DERECHOS: i64 = -3;
const E_CERRADO: i64 = -4;
const E_TAMANO: i64 = -5;
const E_MEMORIA: i64 = -7;
const E_LIMITE: i64 = -8;
const VACIA: u64 = 0x10_0000_0000; // 64 GB: nada proyectado
const KERNEL: u64 = 0xffff_0000_0000_0000;

fn x0(n: u64, a: [u64; 5]) -> i64 {
    llamada(n, a).0 as i64
}

fn principal() -> u64 {
    // 1-3: escribir desde memoria vacía, desde el kernel, y un tramo que da la vuelta.
    if x0(1, [VACIA, 16, 0, 0, 0]) != E_ARGUMENTO {
        return 1;
    }
    if x0(1, [KERNEL, 16, 0, 0, 0]) != E_ARGUMENTO {
        return 2;
    }
    if x0(1, [u64::MAX - 4, 16, 0, 0, 0]) != E_ARGUMENTO {
        return 3;
    }
    let (a, b) = dashos::canal_crear();
    // 4: mandar datos desde memoria vacía.
    if x0(4, [a as u64, VACIA, 10, 0, 0]) != E_ARGUMENTO {
        return 4;
    }
    // 5: un mensaje con 20 bytes y una llave de memoria.
    let m = dashos::memoria_crear(4096);
    let texto = *b"veinte bytes exactos";
    if dashos::canal_escribir(a, &texto, &[m]) != 0 {
        return 5;
    }
    let mut buf = [0u8; 64];
    let mut llaves = [0u32; 4];
    // 6: búfer pequeño: E_TAMANO y el mensaje se queda.
    if x0(5, [b as u64, buf.as_mut_ptr() as u64, 4, llaves.as_mut_ptr() as u64, 4]) != E_TAMANO {
        return 6;
    }
    // 7: leer encima de su propio código, que en EL0 es de solo lectura: el kernel no puede escribir ahí por él.
    let codigo = principal as *const () as u64;
    if x0(5, [b as u64, codigo, 64, llaves.as_mut_ptr() as u64, 4]) != E_ARGUMENTO {
        return 7;
    }
    // 8: datos bien, pero la tabla de llaves en memoria vacía: tampoco se entrega.
    if x0(5, [b as u64, buf.as_mut_ptr() as u64, 64, VACIA, 4]) != E_ARGUMENTO {
        return 8;
    }
    // 9: una capacidad de llaves que desborda al multiplicar por 4.
    if x0(5, [b as u64, buf.as_mut_ptr() as u64, 64, llaves.as_mut_ptr() as u64, u64::MAX / 2]) != E_ARGUMENTO {
        return 9;
    }
    // 10: ahora sí: el mensaje entero, con su llave, que sirve.
    let (n, k) = dashos::canal_leer(b, &mut buf, &mut llaves);
    if n != 20 || k != 1 || buf[..20] != texto {
        return 10;
    }
    if dashos::memoria_proyectar(llaves[0], 0x20_0000_0000, true) != 0 {
        return 11;
    }
    // 12: la misma llave dos veces en un mensaje (hallazgo 6): error, no pánico, y la llave sigue siendo suya.
    let m2 = dashos::memoria_crear(4096);
    if dashos::canal_escribir(a, b"x", &[m2, m2]) != E_ARGUMENTO {
        return 12;
    }
    if dashos::memoria_proyectar(m2, 0x20_0001_0000, true) != 0 {
        return 13;
    }
    // 14: mandar una llave a un canal cerrado: E_CERRADO, y la llave vuelve a su número.
    let m3 = dashos::memoria_crear(4096);
    if x0(6, [b as u64, 0, 0, 0, 0]) != 0 {
        return 14;
    }
    if dashos::canal_escribir(a, b"x", &[m3]) != E_CERRADO {
        return 15;
    }
    if dashos::memoria_proyectar(m3, 0x20_0002_0000, true) != 0 {
        return 16;
    }
    // 17-18: proyectar en la dirección 0 y encima de algo ya proyectado (la página de m3, paso 16).
    if dashos::memoria_proyectar(m2, 0, true) != E_ARGUMENTO {
        return 17;
    }
    if dashos::memoria_proyectar(m2, 0x20_0002_0000, true) != E_ARGUMENTO {
        return 18;
    }
    // 19: la cola de un canal tiene límite (hallazgo 8): el mensaje 17 sin leer da E_LIMITE.
    let (c, d) = dashos::canal_crear();
    for _ in 0..16 {
        if dashos::canal_escribir(c, b"lleno", &[]) != 0 {
            return 19;
        }
    }
    if dashos::canal_escribir(c, b"lleno", &[]) != E_LIMITE {
        return 20;
    }
    let _ = d;
    // 21-22: comerse la memoria a trozos de 4 MB (el tope por objeto era 4 MB desde C35; ahora 16). Tiene que acabar en
    // E_MEMORIA, no en pánico; y al soltarla, se puede volver a pedir. 28: más de 16 MB de una vez da E_TAMANO; 16, no.
    let mut trozos = [0u32; 900];
    let mut n = 0;
    let fin = loop {
        let k = x0(7, [4 << 20, 0, 0, 0, 0]);
        if k < 0 || n == trozos.len() {
            break k;
        }
        trozos[n] = k as u32;
        n += 1;
    };
    if fin != E_MEMORIA {
        return 21;
    }
    for &k in &trozos[..n] {
        x0(6, [k as u64, 0, 0, 0, 0]);
    }
    if x0(7, [(16 << 20) + 4096, 0, 0, 0, 0]) != E_TAMANO {
        return 28;
    }
    let k = x0(7, [16 << 20, 0, 0, 0, 0]);
    if k < 0 {
        return 28;
    }
    x0(6, [k as u64, 0, 0, 0, 0]);
    let k = x0(7, [4 << 20, 0, 0, 0, 0]);
    if k < 0 {
        return 22;
    }
    x0(6, [k as u64, 0, 0, 0, 0]);
    // 23: llenar la tabla de llaves con canales: acaba en E_LIMITE, no en pánico.
    let mut canales = 0;
    let fin = loop {
        let r = x0(3, [0; 5]);
        if r < 0 || canales > 2000 {
            break r;
        }
        canales += 1;
    };
    if fin != E_LIMITE {
        return 23;
    }
    // 24: una cadena de canales, cada uno con los dos extremos del siguiente en su cola (hallazgo 3). Al cerrar
    // el primero se suelta toda; por recursión desbordaba la pila del kernel. Primero se vacía la tabla del paso 23.
    for k in 1..=1100u64 {
        x0(6, [k, 0, 0, 0, 0]);
    }
    // Los canales vivos de un proceso cuentan aunque no estén en su tabla: desde C42 se cobran a su grupo, y la cadena
    // para en E_MEMORIA cuando se acaba el cupo (antes, en E_LIMITE a los 512).
    let (mut a, mut b) = dashos::canal_crear();
    let mut largo = 1;
    let fin = loop {
        let r = dashos::llamada(3, [0; 5]);
        if (r.0 as i64) < 0 || largo > 5000 {
            break r.0 as i64;
        }
        let (c, d) = (r.0 as u32, r.1 as u32);
        if dashos::canal_escribir(c, b"cadena", &[a, b]) != 0 {
            return 24;
        }
        (a, b) = (c, d);
        largo += 1;
    };
    if fin != E_MEMORIA {
        return 26;
    }
    x0(6, [a as u64, 0, 0, 0, 0]);
    x0(6, [b as u64, 0, 0, 0, 0]);
    // 32-33 (S4 de docs/REVISION_ARQUITECTURA.md): una llave que se puede transferir una vez llega por un canal y ya
    // no se puede pasar; su copia hereda la restricción.
    let mu = dashos::memoria_crear(4096);
    let una = dashos::llave_duplicar(mu, dashos::LEER | dashos::TRANSFERIR | dashos::UNA_VEZ, 0) as u32;
    let (a, b) = dashos::canal_crear();
    if dashos::canal_escribir(a, b"una vez", &[una]) != 0 {
        return 32;
    }
    let mut buf = [0u8; 16];
    let mut ll = [0u32; 1];
    let (leido, k) = dashos::canal_leer(b, &mut buf, &mut ll);
    if leido != 7 || k != 1 || dashos::canal_escribir(a, b"otra", &[ll[0]]) != E_DERECHOS {
        return 32;
    }
    let copia = dashos::llave_duplicar(ll[0], dashos::LEER | dashos::TRANSFERIR, 0);
    if copia <= 0 || dashos::canal_escribir(a, b"copia", &[copia as u32]) != E_DERECHOS {
        return 33;
    }
    for l in [ll[0], copia as u32, a, b, mu] {
        dashos::cerrar(l);
    }
    // 34 (S3): sin el permiso, la clase de plazo no se da; volver a la justa, sí.
    if dashos::plazo(0, 1_000, 10_000) >= 0 || dashos::plazo(1, 1_000, 10_000) >= 0 || dashos::plazo(0, 0, 0) != 0 {
        return 34;
    }
    // 35 (K7): leer con plazo. Sin nada que leer, E_PLAZO pasado el plazo y no antes; con un mensaje, lo lee.
    let (a, b) = dashos::canal_crear();
    let t0 = dashos::ahora_us();
    let (r, _, _) = dashos::canal_leer_plazo(b, &mut buf, &mut [], 3_000);
    let us = dashos::ahora_us() - t0;
    dashos::canal_escribir(a, b"hola", &[]);
    let (r2, _, _) = dashos::canal_leer_plazo(b, &mut buf, &mut [], 3_000);
    if r != dashos::E_PLAZO || us < 3_000 || r2 != 4 {
        return 35;
    }
    // Un plazo enorme no desborda: el otro lado cerrado sigue dando E_CERRADO.
    dashos::cerrar(a);
    if dashos::canal_leer_plazo(b, &mut buf, &mut [], u64::MAX >> 16).0 != E_CERRADO {
        return 35;
    }
    dashos::cerrar(b);
    // 36 (K7): un aviso de fin sobre algo que no es un proceso, o sin canal, se rechaza.
    let (a, b) = dashos::canal_crear();
    if dashos::proceso_avisar(a, b) != E_TIPO || dashos::proceso_avisar(999, b) != E_LLAVE {
        return 36;
    }
    dashos::cerrar(a);
    dashos::cerrar(b);
    // 37 (H4, el JIT de la caja Amiga): escribir y ejecutar por la misma dirección se rechaza, con cualquier
    // combinación de bits; el mismo objeto, escrito por una dirección y ejecutado por otra, funciona, también después
    // de reescribirlo; y el kernel no escribe por el proceso en la dirección de ejecutar.
    let paso = jit();
    if paso != 0 {
        println!("  [malo] JIT: falla el paso {}", paso);
        return 37;
    }
    // 38 (H4, la página 0 de la caja Amiga): sin el permiso de página 0, la llamada 49 no proyecta nada (una llave que
    // no existe, una de memoria en su lugar, un canal); y por `memoria_proyectar`, ni la dirección 0 ni nada de los
    // primeros 64 KB, tampoco de solo lectura.
    let paso = pagina_cero();
    if paso != 0 {
        println!("  [malo] página 0: falla el paso {}", paso);
        return 38;
    }
    // 39 (H9, vistas de memoria para el compositor): la llamada 50 con páginas fuera, llaves que no son suyas o no son
    // de memoria, sin derecho de leer, revocables, vistas de vistas, tablas malas y listas enormes; solo se lee; y el
    // origen soltado con la vista viva no se reparte a otro.
    let paso = vista();
    if paso != 0 {
        println!("  [malo] vistas: falla el paso {}", paso);
        return 39;
    }
    // 27: PAN por software (C34). Dentro del kernel, fuera de las copias, la memoria del proceso no se ve.
    let local = 0u64;
    if x0(99, [&local as *const u64 as u64, 0, 0, 0, 0]) != 0 {
        return 27;
    }
    // 25: una secuencia de escape no llega al terminal: sale "?" en lugar del ESC.
    dashos::escribir("  [malo] escape: \x1b[31m\n");
    // 26 (auditoría de la fase 4, B2): media línea, una línea del kernel (una llamada que no existe) y después algo que
    // quiere parecer del kernel. Tiene que salir con la marca "│ " delante.
    dashos::escribir("x");
    dashos::llamada(77, [0; 5]);
    dashos::escribir("C99: imitado por un programa\n");
    println!("  [malo] memoria agotada en {} trozos de 4 MB, tabla llena en {} canales y cadena de {} canales, sin pánico", n, canales, largo);
    println!("  [malo] punteros malos, llaves repetidas y canales cerrados: todo rechazado sin perder nada");
    31
}

/// El paso 37: W^X con un JIT (H4). 0 si todo va bien; si no, qué falló.
fn jit() -> u32 {
    const ESCRIBIR_EN: usize = 0x30_0000_0000; // 192 GB: lo de antes usa 128 GB
    const EJECUTAR_EN: usize = 0x31_0000_0000;
    let m = dashos::memoria_crear(4096);
    // Escribir y ejecutar a la vez: ni 6 ni 7; y nada queda proyectado.
    for p in [6u64, 7, 0xff] {
        if x0(8, [m as u64, EJECUTAR_EN as u64, p, 0, 0]) != E_ARGUMENTO {
            return 1;
        }
    }
    if dashos::memoria_proyectar(m, ESCRIBIR_EN, true) != 0 || dashos::memoria_proyectar_codigo(m, EJECUTAR_EN) != 0 {
        return 2;
    }
    // Con una llave de solo leer también se puede ejecutar (lo que se puede leer ya se puede copiar a otro sitio).
    let solo_leer = dashos::llave_duplicar(m, dashos::LEER, 0);
    if solo_leer <= 0 || dashos::memoria_proyectar_codigo(solo_leer as u32, EJECUTAR_EN + 0x10_0000) != 0 {
        return 3;
    }
    // "mov x0, #n; ret", escrito por una dirección y ejecutado por la otra, dos veces con n distinto.
    for n in [42u32, 43] {
        // Lo mínimo para un JIT: escribir las instrucciones (siempre little-endian), limpiar y saltar a ellas.
        #[cfg(target_arch = "aarch64")]
        let r = unsafe {
            let codigo = [0xd280_0000 | (n << 5), 0xd65f_03c0u32];
            for (i, w) in codigo.iter().enumerate() {
                ((ESCRIBIR_EN + 4 * i) as *mut u32).write_volatile(w.to_le());
            }
            core::arch::asm!("dc cvau, {e}", "dsb ish", "ic ivau, {x}", "dsb ish", "isb", e = in(reg) ESCRIBIR_EN, x = in(reg) EJECUTAR_EN);
            let f: extern "C" fn() -> u64 = core::mem::transmute(EJECUTAR_EN);
            f()
        };
        // x86-64: "mov eax, n; ret" (B8 y cuatro bytes, C3). No hay caché de instrucciones que limpiar, pero el código
        // se escribió por otra dirección lineal que la de ejecutarlo: antes de saltar, una instrucción que serializa,
        // como CPUID (SDM, volumen 3, "Self-Modifying Code").
        #[cfg(target_arch = "x86_64")]
        let r = unsafe {
            let b = n.to_le_bytes();
            for (i, w) in [0xb8, b[0], b[1], b[2], b[3], 0xc3].iter().enumerate() {
                ((ESCRIBIR_EN + i) as *mut u8).write_volatile(*w);
            }
            let _ = core::arch::x86_64::__cpuid(0);
            let f: extern "C" fn() -> u64 = core::mem::transmute(EJECUTAR_EN);
            f()
        };
        if r != n as u64 {
            return 4;
        }
    }
    // El kernel escribe con los permisos de EL0: por la dirección de ejecutar, no.
    let (a, b) = dashos::canal_crear();
    dashos::canal_escribir(a, b"codigo", &[]);
    let r = x0(5, [b as u64, EJECUTAR_EN as u64, 16, 0, 0]);
    for l in [a, b, solo_leer as u32] {
        dashos::cerrar(l);
    }
    dashos::memoria_quitar(ESCRIBIR_EN);
    dashos::memoria_quitar(EJECUTAR_EN);
    dashos::memoria_quitar(EJECUTAR_EN + 0x10_0000);
    dashos::cerrar(m);
    if r != E_ARGUMENTO {
        return 5;
    }
    println!("  [malo] JIT: se escribe por una dirección y se ejecuta por otra; escribir y ejecutar a la vez, rechazado");
    0
}

/// El paso 38: la página 0 (H4). 0 si todo va bien; si no, qué falló.
fn pagina_cero() -> u32 {
    let m = dashos::memoria_crear(4096);
    let (a, b) = dashos::canal_crear();
    if x0(49, [999, m as u64, 0, 0, 0]) != E_LLAVE {
        return 1;
    }
    if x0(49, [m as u64, m as u64, 0, 0, 0]) != E_TIPO || x0(49, [a as u64, m as u64, 0, 0, 0]) != E_TIPO {
        return 2;
    }
    for va in [0usize, 0x1000, 0x8000, 0xf000] {
        if dashos::memoria_proyectar(m, va, false) != E_ARGUMENTO || dashos::memoria_proyectar_codigo(m, va) != E_ARGUMENTO {
            return 3;
        }
    }
    // Nada quedó proyectado en la dirección 0.
    if dashos::memoria_quitar(0) != E_ARGUMENTO {
        return 4;
    }
    for l in [m, a, b] {
        dashos::cerrar(l);
    }
    println!("  [malo] página 0: sin permiso, ni la llamada 49 ni los primeros 64 KB, tampoco de solo lectura");
    0
}

/// El paso 39: vistas de memoria (llamada 50, H9). 0 si todo va bien; si no, qué falló.
fn vista() -> u32 {
    const ORIGEN_EN: usize = 0x32_0000_0000;
    const VISTA_EN: usize = 0x33_0000_0000;
    const OTRA_EN: usize = 0x34_0000_0000;
    let vista = |o: &[u32], p: &[u32]| dashos::memoria_vista(o, p);
    let m = dashos::memoria_crear(3 * 4096);
    if dashos::memoria_proyectar(m, ORIGEN_EN, true) != 0 {
        return 1;
    }
    // Cada página empieza por su número.
    for p in 0..3 {
        unsafe { ((ORIGEN_EN + p * 4096) as *mut u64).write_volatile(0x5649_5354_0000 + p as u64) };
    }
    // Una vista con las páginas 2 y 0: se lee en ese orden, y mide dos páginas.
    let v = vista(&[m], &[2, 0]);
    if v <= 0 || dashos::memoria_tamano(v as u32) != 8192 || dashos::memoria_proyectar(v as u32, VISTA_EN, false) != 0 {
        return 2;
    }
    let v = v as u32;
    let leer = |va: usize| unsafe { (va as *const u64).read_volatile() };
    if leer(VISTA_EN) != 0x5649_5354_0002 || leer(VISTA_EN + 4096) != 0x5649_5354_0000 {
        return 3;
    }
    // Lo que escribe el dueño se ve por la vista, sin copias.
    unsafe { ((ORIGEN_EN + 2 * 4096 + 8) as *mut u64).write_volatile(0x1234) };
    if leer(VISTA_EN + 8) != 0x1234 {
        return 4;
    }
    // Solo lectura: ni escribir (la llave no tiene el derecho, ni pidiéndolo al duplicar) ni ejecutar.
    let mas = dashos::llave_duplicar(v, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    if dashos::memoria_proyectar(v, OTRA_EN, true) != E_DERECHOS
        || mas <= 0
        || dashos::memoria_proyectar(mas as u32, OTRA_EN, true) != E_DERECHOS
        || dashos::memoria_proyectar_codigo(v, OTRA_EN) != E_ARGUMENTO
    {
        return 5;
    }
    dashos::cerrar(mas as u32);
    // Páginas fuera del origen.
    if vista(&[m], &[3]) != E_ARGUMENTO || vista(&[m], &[0, u32::MAX]) != E_ARGUMENTO {
        return 6;
    }
    // Llaves que no son suyas, que no son de memoria, sin derecho de leer, revocables, o una vista como origen.
    let (a, b) = dashos::canal_crear();
    let sin_leer = dashos::llave_duplicar(m, dashos::TRANSFERIR, 0) as u32;
    let (revocable, revocador) = dashos::llave_revocable(m);
    if vista(&[999], &[0]) != E_LLAVE
        || vista(&[a], &[0]) != E_TIPO
        || vista(&[sin_leer], &[0]) != E_DERECHOS
        || vista(&[m, sin_leer], &[0]) != E_DERECHOS
        || revocable <= 0
        || vista(&[revocable as u32], &[0]) != E_ARGUMENTO
        || vista(&[v], &[0]) != E_ARGUMENTO
    {
        return 7;
    }
    for l in [a, b, sin_leer, revocable as u32, revocador] {
        dashos::cerrar(l);
    }
    // Tablas malas y listas enormes: sin orígenes o sin páginas, de más, en memoria vacía, en el kernel o que desbordan.
    let paginas = [0u32; 4097];
    let origenes = [m; 65];
    let mal = [
        (x0(50, [origenes.as_ptr() as u64, 0, paginas.as_ptr() as u64, 1, 0]), E_TAMANO),
        (x0(50, [origenes.as_ptr() as u64, 1, paginas.as_ptr() as u64, 0, 0]), E_TAMANO),
        (x0(50, [origenes.as_ptr() as u64, 65, paginas.as_ptr() as u64, 1, 0]), E_TAMANO),
        (x0(50, [origenes.as_ptr() as u64, 1, paginas.as_ptr() as u64, 4097, 0]), E_TAMANO),
        (x0(50, [origenes.as_ptr() as u64, 1, paginas.as_ptr() as u64, u64::MAX / 2, 0]), E_TAMANO),
        (x0(50, [VACIA, 1, paginas.as_ptr() as u64, 1, 0]), E_ARGUMENTO),
        (x0(50, [origenes.as_ptr() as u64, 1, VACIA, 1, 0]), E_ARGUMENTO),
        (x0(50, [KERNEL, 1, paginas.as_ptr() as u64, 1, 0]), E_ARGUMENTO),
        (x0(50, [origenes.as_ptr() as u64, 1, u64::MAX - 8, 4, 0]), E_ARGUMENTO),
    ];
    if mal.iter().any(|(r, e)| r != e) {
        return 8;
    }
    // La lista más larga (16 MB de vista, la misma página 4096 veces) y 64 orígenes iguales sí valen.
    let grande = x0(50, [origenes.as_ptr() as u64, 64, paginas.as_ptr() as u64, 4096, 0]);
    if grande <= 0 || dashos::memoria_tamano(grande as u32) != 16 << 20 {
        return 9;
    }
    dashos::cerrar(grande as u32);
    // Muchas vistas sin cerrar: el cupo o la tabla las cortan (sin pánico), y al cerrarlas vuelve todo.
    let mut hechas = [0u32; 1100];
    let mut cuantas = 0;
    loop {
        let l = vista(&[m], &paginas[..4096]);
        if l < 0 {
            if l != E_MEMORIA && l != E_LIMITE {
                return 10;
            }
            break;
        }
        if cuantas == hechas.len() {
            return 11;
        }
        hechas[cuantas] = l as u32;
        cuantas += 1;
    }
    for &l in &hechas[..cuantas] {
        dashos::cerrar(l);
    }
    // El origen se suelta con la vista viva: sus páginas siguen siendo suyas. La memoria que se pide después no las
    // recibe (se llena entera de otra cosa y la vista no cambia).
    dashos::memoria_quitar(ORIGEN_EN);
    dashos::cerrar(m);
    let mut nuevas = [0u32; 16];
    for (i, nueva) in nuevas.iter_mut().enumerate() {
        let n = dashos::memoria_crear(64 * 1024);
        if dashos::memoria_proyectar(n, OTRA_EN + i * 0x10_0000, true) != 0 {
            return 12;
        }
        for k in (0..64 * 1024).step_by(8) {
            unsafe { ((OTRA_EN + i * 0x10_0000 + k) as *mut u64).write_volatile(0xeeee_eeee_eeee_eeee) };
        }
        *nueva = n;
    }
    if leer(VISTA_EN) != 0x5649_5354_0002 || leer(VISTA_EN + 8) != 0x1234 || leer(VISTA_EN + 4096) != 0x5649_5354_0000 {
        return 13;
    }
    for (i, &n) in nuevas.iter().enumerate() {
        dashos::memoria_quitar(OTRA_EN + i * 0x10_0000);
        dashos::cerrar(n);
    }
    dashos::memoria_quitar(VISTA_EN);
    dashos::cerrar(v);
    println!("  [malo] vistas: solo de lectura, páginas fuera, llaves ajenas o sin derecho, tablas malas y {} vistas de 16 MB: rechazado lo que no vale; el origen suelto no se reparte", cuantas);
    0
}
