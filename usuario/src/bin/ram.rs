//! `RAM:` en memoria (docs/bitacoras/asignaciones.md): el programa de la prueba en `virt`. Está en el disco
//! (`Programas/ram`) y `guardian` lo lanza tres veces; hace un papel u otro según de quién sea.
//!
//! - **Ana**: su `ENV:` trae lo de `ENVARC:` y cambiarlo no toca `ENVARC:`. Deja un secreto en su `RAM:`. Y la llena
//!   hasta el tope con datos que no se comprimen: tiene que dar "lleno" sin tumbar nada, la memoria libre de la máquina
//!   baja lo escrito y, al borrar, vuelve.
//! - **Carla, la primera vez** (con la sesión de Ana abierta): en su `RAM:` no está el secreto de Ana ni llega a él por
//!   ningún sitio; las órdenes de los volúmenes en memoria no son para un programa. Deja una huella en `T:`, cambia su
//!   `ENV:` y guarda en su `Home:` una capa suya de `ENVARC:`.
//! - **Carla, la segunda** (`guardian` cerró su sesión y la volvió a abrir): la huella y el cambio de `ENV:` no están;
//!   `ENV:` trae ahora su capa encima de la de fábrica, y cambiar `ENV:` no toca lo guardado.
//!
//! Llave 1: su canal con `sesiones`; llave 2: su llave de archivos (las dos, marcadas con su número).

#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use dashos::asignaciones::{self as asig, Tabla};
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;

dashos::programa!(principal);
dashos::monton!();

const SESIONES: u32 = 1;
const ARCHIVOS: u32 = 2;
const VA_VENTANA: usize = 0x40_0000_0000;
const RAM: u32 = s::RAIZ_RAM as u32;
const HOME: u32 = s::RAIZ_HOME as u32;

/// Su sesión con el servicio de archivos, con la ventana más grande (1 MB): llenar `RAM:` va de mega en mega.
struct Archivos {
    lectura: u32,
    tam: usize,
}

impl Archivos {
    fn abrir() -> Option<Archivos> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        p[8..16].copy_from_slice(&(a::VENTANA_MAX as u64).to_le_bytes());
        if dashos::canal_escribir(ARCHIVOS, &p, &[suyo]) != 0 {
            return None;
        }
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 3_000_000);
        if n != 32 || k != 1 || i32::from_le_bytes(r[0..4].try_into().unwrap()) != a::BIEN || dashos::memoria_proyectar(ll[0], VA_VENTANA, true) != 0 {
            return None;
        }
        Some(Archivos { lectura, tam: u64::from_le_bytes(r[8..16].try_into().unwrap()) as usize })
    }

    fn orden(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> asig::Resp {
        let mut p = [0u8; 256];
        let largo = nombre.len().min(224);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(largo as u32).to_le_bytes());
        p[32..32 + largo].copy_from_slice(&nombre[..largo]);
        let mut r = [0u8; 32];
        // Llenar un mega con datos que no se comprimen tarda en QEMU: 30 s de plazo.
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + largo], &[]) != 0 || dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 30_000_000).0 != 32 {
            return asig::Resp { e: i32::MIN, ..Default::default() };
        }
        let u = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
        asig::Resp { e: i32::from_le_bytes(r[0..4].try_into().unwrap()), asa: u32::from_le_bytes(r[4..8].try_into().unwrap()), a: u(8), b: u(16), c: u(24) }
    }

    fn ventana(&self) -> &'static mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, self.tam) }
    }

    fn cerrar(&self, asa: u32) {
        self.orden(a::CERRAR, asa, 0, 0, b"");
    }

    /// Si `camino` existe desde la raíz `raiz` (sin abrirlo): el estado.
    fn esta(&self, raiz: u32, camino: &[u8]) -> i32 {
        self.orden(a::BUSCAR, raiz, 1, 0, camino).e
    }

    /// Crea (o deja, si ya está) el cajón `nombre` en la asa `en` y lo abre.
    fn cajon(&self, en: u32, nombre: &[u8]) -> Result<u32, i32> {
        let r = self.orden(a::CREAR, en, a::CAJON as u64, 0, nombre);
        let r = if r.e == a::YA_EXISTE { self.orden(a::BUSCAR, en, 0, 0, nombre) } else { r };
        if r.e == a::BIEN { Ok(r.asa) } else { Err(r.e) }
    }

    /// Deja `datos` en el fichero `nombre` de la asa `en` (lo crea o lo recorta).
    fn poner(&self, en: u32, nombre: &[u8], datos: &[u8]) -> i32 {
        let r = self.orden(a::CREAR, en, a::FICHERO as u64, 0, nombre);
        let r = if r.e == a::YA_EXISTE { self.orden(a::BUSCAR, en, 0, 0, nombre) } else { r };
        if r.e != a::BIEN {
            return r.e;
        }
        let e = self.escribir(r.asa, datos);
        self.cerrar(r.asa);
        e
    }

    /// Recorta el fichero de la asa y le escribe `datos`.
    fn escribir(&self, asa: u32, datos: &[u8]) -> i32 {
        let e = self.orden(a::TAMANO, asa, 0, 0, b"").e;
        if e != a::BIEN {
            return e;
        }
        self.ventana()[..datos.len()].copy_from_slice(datos);
        self.orden(a::ESCRIBIR, asa, 0, datos.len() as u64, b"").e
    }

    /// Lo que hay (hasta 4 KB) en el fichero de la asa, que se cierra.
    fn leer(&self, asa: u32) -> Result<Vec<u8>, i32> {
        let r = self.orden(a::LEER, asa, 0, 4096, b"");
        let datos = (r.e == a::BIEN).then(|| self.ventana()[..(r.a as usize).min(4096)].to_vec());
        self.cerrar(asa);
        datos.ok_or(r.e)
    }
}

/// El servicio de archivos para `dashos::asignaciones`.
struct PorNombres<'a>(&'a Archivos);

impl asig::Fs for PorNombres<'_> {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> asig::Resp {
        self.0.orden(orden, asa, x, y, nombre)
    }

    fn ventana(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(VA_VENTANA as *const u8, a::VENTANA) }
    }
}

/// Lo que hay en el fichero `camino` (con nombre: `ENV:Sys/idioma`), o el estado.
fn leer(arch: &Archivos, t: &Tabla, camino: &[u8]) -> Result<Vec<u8>, i32> {
    let (r, _) = asig::abrir(&mut PorNombres(arch), t, camino)?;
    arch.leer(r.asa)
}

/// Escribe `datos` en el fichero `camino`, que ya existe (con nombre): el estado.
fn cambiar(arch: &Archivos, t: &Tabla, camino: &[u8], datos: &[u8]) -> i32 {
    match asig::abrir(&mut PorNombres(arch), t, camino) {
        Ok((r, _)) => {
            let e = arch.escribir(r.asa, datos);
            arch.cerrar(r.asa);
            e
        }
        Err(e) => e,
    }
}

/// Bytes que no se comprimen (DashFS comprime con LZ4): lo escrito ocupa lo que mide.
fn ruido(destino: &mut [u8], semilla: &mut u64) {
    for trozo in destino.chunks_mut(8) {
        *semilla ^= *semilla << 13;
        *semilla ^= *semilla >> 7;
        *semilla ^= *semilla << 17;
        trozo.copy_from_slice(&semilla.to_le_bytes()[..trozo.len()]);
    }
}

/// Lo que mide y lo que tiene libre su `RAM:`, en KB.
fn sitio(arch: &Archivos) -> (u64, u64) {
    let r = arch.orden(a::INFO, RAM, 0, 0, b"");
    (r.a / 1024, r.b / 1024)
}

/// KB libres para procesos en la máquina (llamada 29, contador 15).
fn libres() -> i64 {
    dashos::medir(15, 0) / 1024
}

fn principal() -> u64 {
    let Some(arch) = Archivos::abrir() else {
        println!("  [ram] sin sesión en archivos: MAL");
        return 1;
    };
    let Some(t) = asig::pedir_tabla(SESIONES) else {
        println!("  [ram] sin tabla de nombres: MAL");
        return 1;
    };
    // Quién es: el usuario de quien pregunta, que lo dice el servicio de archivos.
    let bien = match arch.orden(a::DUENO, 0, 0, 0, b"").c {
        1000 => ana(&arch, &t),
        1001 if arch.esta(HOME, b"ram.fase") != a::BIEN => carla_primera(&arch, &t),
        1001 => carla_segunda(&arch, &t),
        otro => {
            println!("  [ram] lanzado como el usuario {}: MAL", otro);
            false
        }
    };
    if bien { 0 } else { 1 }
}

fn ana(arch: &Archivos, t: &Tabla) -> bool {
    // `RAM:` nace con `T` y `Env`, y `ENV:` con lo de `ENVARC:` (de fábrica: Ana no tiene capa suya).
    let cajones = arch.esta(RAM, b"T") == a::BIEN && arch.esta(RAM, b"Env/Sys") == a::BIEN;
    let de_fabrica = leer(arch, t, b"ENV:Sys/idioma");
    let cambio = cambiar(arch, t, b"ENV:Sys/idioma", b"xx\n");
    let (en_env, en_envarc) = (leer(arch, t, b"ENV:Sys/idioma"), leer(arch, t, b"ENVARC:Sys/idioma"));
    let env = cajones && de_fabrica.as_deref() == Ok(b"es\n") && cambio == a::BIEN && en_env.as_deref() == Ok(b"xx\n") && en_envarc.as_deref() == Ok(b"es\n");
    println!(
        "  [ram] Ana: RAM: con T y Env {}; ENV:Sys/idioma trae lo de ENVARC: {}; cambiado en ENV: ({}), ENVARC: sigue igual {}: {}",
        cajones,
        de_fabrica.as_deref() == Ok(b"es\n"),
        cambio,
        en_envarc.as_deref() == Ok(b"es\n"),
        if env { "bien" } else { "MAL" }
    );
    // El secreto que Carla no tiene que ver.
    let secreto = arch.poner(RAM, b"secreto.txt", b"lo de Ana en su RAM:\n");
    // Llenarla: de mega en mega y, cuando no cabe uno, de 64 KB en 64 KB, hasta que diga que no.
    let (mide, libre_antes) = sitio(arch);
    let maquina_antes = libres();
    let t0 = dashos::ahora_us();
    let relleno = arch.orden(a::CREAR, RAM, a::FICHERO as u64, 0, b"relleno");
    let mut semilla = 0x9E37_79B9_7F4A_7C15u64;
    let (mut escrito, mut fin) = (0u64, relleno.e);
    for trozo in [arch.tam.min(1 << 20), 64 << 10] {
        while fin == a::BIEN && escrito < 2 * mide * 1024 {
            ruido(&mut arch.ventana()[..trozo], &mut semilla);
            fin = arch.orden(a::ESCRIBIR, relleno.asa, escrito, trozo as u64, b"").e;
            if fin == a::BIEN {
                escrito += trozo as u64;
            }
        }
        if fin == a::LLENO && trozo > 64 << 10 {
            fin = a::BIEN;
        }
    }
    let ms = (dashos::ahora_us() - t0) / 1000;
    let maquina_llena = libres();
    let (_, libre_llena) = sitio(arch);
    // Llena: escribir en otro fichero también da "lleno"; lo que había se lee, y el servicio sigue atendiendo a todos.
    let otro = arch.orden(a::CREAR, RAM, a::FICHERO as u64, 0, b"otro");
    let otro = if otro.e == a::BIEN {
        ruido(&mut arch.ventana()[..256 << 10], &mut semilla);
        let e = arch.orden(a::ESCRIBIR, otro.asa, 0, 256 << 10, b"").e;
        arch.cerrar(otro.asa);
        arch.orden(a::BORRAR, RAM, 0, 0, b"otro");
        e
    } else {
        otro.e
    };
    let sigue = arch.orden(a::BUSCAR, RAM, 0, 0, b"secreto.txt");
    let leido = if sigue.e == a::BIEN { arch.leer(sigue.asa) } else { Err(sigue.e) };
    let sistema = arch.esta(s::RAIZ_SYS as u32, b"C/grande.txt");
    // Borrar, llena: se puede, y la memoria vuelve.
    arch.cerrar(relleno.asa);
    let borrado = arch.orden(a::BORRAR, RAM, 0, 0, b"relleno").e;
    let maquina_despues = libres();
    let (_, libre_despues) = sitio(arch);
    let kb = escrito / 1024;
    let gastado = maquina_antes - maquina_llena;
    let vuelto = maquina_despues - maquina_llena;
    // Cabe casi todo el tope (menos la reserva para borrar y los metadatos), nunca más; la máquina pierde eso (y no
    // más: ni el doble ni la memoria entera) y lo recupera. Con otros programas en marcha, un margen de 4 MB.
    let llena = relleno.e == a::BIEN && fin == a::LLENO && otro == a::LLENO && kb <= mide && kb + 2048 >= mide && libre_llena < 256
        && leido.as_deref() == Ok(b"lo de Ana en su RAM:\n") && sistema == a::BIEN;
    let memoria = (gastado - kb as i64).abs() < 4096 && (vuelto - kb as i64).abs() < 4096 && borrado == a::BIEN && libre_despues + 512 >= libre_antes;
    println!(
        "  [ram] Ana: RAM: de {} MB: escritos {} KB en {} ms hasta que da {} (lleno, -11); escribir en otro {}; lo de antes se lee {} y el disco también ({}): {}",
        mide / 1024,
        kb,
        ms,
        fin,
        otro,
        leido.is_ok(),
        sistema,
        if llena { "bien" } else { "MAL" }
    );
    println!(
        "  [ram] Ana: memoria libre de la máquina: {} KB antes, {} llena (baja {}), {} tras borrar ({}; vuelven {}); libre en RAM: {} KB antes, {} llena, {} después: {}",
        maquina_antes,
        maquina_llena,
        gastado,
        maquina_despues,
        borrado,
        vuelto,
        libre_antes,
        libre_llena,
        libre_despues,
        if memoria { "bien" } else { "MAL" }
    );
    let bien = env && secreto == a::BIEN && llena && memoria;
    println!("  [ram] Ana: ENV: de ENVARC:, RAM: llena sin tumbar nada y la memoria que vuelve al borrar: {}", if bien { "bien" } else { "MAL" });
    bien
}

/// Cuántas entradas tiene la raíz de su `RAM:`.
fn en_la_raiz(arch: &Archivos) -> u64 {
    let r = arch.orden(a::LISTAR, RAM, 0, 0, b"");
    if r.e == a::BIEN { r.a } else { u64::MAX }
}

fn carla_primera(arch: &Archivos, t: &Tabla) -> bool {
    // Su `RAM:` es otra: recién hecha, con `T` y `Env` y nada de Ana.
    let entradas = en_la_raiz(arch);
    let secreto = arch.esta(RAM, b"secreto.txt");
    // Por ningún sitio: ninguna de sus raíces lleva a la `RAM:` de Ana, ni subiendo, ni por el cajón que tuvo en el
    // disco, ni pidiendo el volumen por un nombre; y las órdenes de los volúmenes en memoria (crear, quitar, dar una
    // raíz, copiar) son de `sesiones`: con la llave de un programa no existen.
    let mut llega = 0;
    for raiz in 0..16 {
        for camino in [&b"secreto.txt"[..], b"../secreto.txt", b"Estado/RAM/Ana/secreto.txt", b"RAM/secreto.txt", b"Ana/secreto.txt"] {
            llega += (arch.esta(raiz, camino) == a::BIEN) as u32;
        }
    }
    let por_volumen = [&b"RAM"[..], b"Ana", b"\xff\xe8\x03\x00\x00"].map(|v| arch.orden(a::VOLUMEN, 0, 0, 0, v).e);
    let mut cabecera = [0u8; 20];
    cabecera[0..4].copy_from_slice(&1000u32.to_le_bytes());
    cabecera[4..8].copy_from_slice(&1001u32.to_le_bytes());
    let de_sesiones = [a::RAM_CREAR, a::RAM_QUITAR, a::RAM_RAIZ, a::RAM_COPIAR].map(|o| arch.orden(o, 1000, 1000, 1001, &cabecera).e);
    let aislada = entradas == 2 && secreto == a::NO_EXISTE && llega == 0 && por_volumen.iter().all(|&e| e != a::BIEN) && de_sesiones.iter().all(|&e| e == a::MAL_FORMADO);
    println!(
        "  [ram] Carla: su RAM: tiene {} entradas (T y Env); el secreto de Ana: {} en la suya, {} caminos llegan a él; pedir el volumen {:?}; órdenes de sesiones {:?}: {}",
        entradas,
        secreto,
        llega,
        por_volumen,
        de_sesiones,
        if aislada { "bien" } else { "MAL" }
    );
    // `ENV:` de fábrica; se cambia una cosa, se deja una huella en `T:` y, en `Home:`, su capa de `ENVARC:` para la
    // próxima vez.
    let fabrica = leer(arch, t, b"ENV:Sys/idioma").as_deref() == Ok(b"es\n") && leer(arch, t, b"ENV:Sys/teclado").as_deref() == Ok(b"es\n");
    let cambio = cambiar(arch, t, b"ENV:Sys/teclado", b"us\n");
    let huella = arch.orden(a::BUSCAR, RAM, 0, 0, b"T");
    let e_huella = if huella.e == a::BIEN { arch.poner(huella.asa, b"huella", b"estuve aqui\n") } else { huella.e };
    arch.cerrar(huella.asa);
    let capa = (|| {
        let prefs = arch.cajon(HOME, b"Prefs")?;
        let envarc = arch.cajon(prefs, b"Env-Archive")?;
        let sys = arch.cajon(envarc, b"Sys")?;
        let escritorio = arch.cajon(envarc, b"Escritorio")?;
        let e = [arch.poner(sys, b"idioma", b"ca\n"), arch.poner(escritorio, b"tema", b"claro\n"), arch.poner(HOME, b"ram.fase", b"2\n")];
        for asa in [escritorio, sys, envarc, prefs] {
            arch.cerrar(asa);
        }
        if e.iter().all(|&x| x == a::BIEN) { Ok(()) } else { Err(e[0].min(e[1]).min(e[2])) }
    })();
    // Lo guardado en `Home:` ya se ve por `ENVARC:` (su capa va delante), pero `ENV:` es la copia de cuando entró.
    let envarc = leer(arch, t, b"ENVARC:Sys/idioma").as_deref() == Ok(b"ca\n") && leer(arch, t, b"ENV:Sys/idioma").as_deref() == Ok(b"es\n");
    let bien = aislada && fabrica && cambio == a::BIEN && e_huella == a::BIEN && capa == Ok(()) && envarc;
    println!(
        "  [ram] Carla, primera sesión: ENV: de fábrica {}; cambia ENV:Sys/teclado {}, deja T:huella {} y guarda su capa de ENVARC: en Home: {:?} (ENV: sigue como al entrar: {}): {}",
        fabrica,
        cambio,
        e_huella,
        capa,
        envarc,
        if bien { "bien" } else { "MAL" }
    );
    bien
}

fn carla_segunda(arch: &Archivos, t: &Tabla) -> bool {
    // Salió y volvió a entrar: `RAM:` es nueva. Ni la huella ni el cambio de `ENV:`.
    let entradas = en_la_raiz(arch);
    let huella = arch.esta(RAM, b"T/huella");
    let teclado = leer(arch, t, b"ENV:Sys/teclado");
    // `ENV:` trae ahora su capa encima de la de fábrica: lo suyo gana donde hay de los dos, y lo que solo es suyo está.
    let idioma = leer(arch, t, b"ENV:Sys/idioma");
    let tema = leer(arch, t, b"ENV:Escritorio/tema");
    // Y cambiar `ENV:` no toca lo guardado: ni por `ENVARC:` ni en su `Home:`.
    let cambio = cambiar(arch, t, b"ENV:Sys/idioma", b"zz\n");
    let en_env = leer(arch, t, b"ENV:Sys/idioma");
    let en_envarc = leer(arch, t, b"ENVARC:Sys/idioma");
    let en_home = arch.orden(a::BUSCAR, HOME, 0, 0, b"Prefs/Env-Archive/Sys/idioma");
    let en_home = if en_home.e == a::BIEN { arch.leer(en_home.asa) } else { Err(en_home.e) };
    // Se deja como estaba, para la próxima prueba sobre el mismo disco.
    let mut limpio = true;
    for (cajon, nombre) in [(&b"Prefs/Env-Archive/Sys"[..], &b"idioma"[..]), (b"Prefs/Env-Archive/Escritorio", b"tema"), (b"Prefs/Env-Archive", b"Sys"), (b"Prefs/Env-Archive", b"Escritorio"), (b"Prefs", b"Env-Archive"), (b"", b"Prefs"), (b"", b"ram.fase")] {
        let r = arch.orden(a::BUSCAR, HOME, 0, 0, cajon);
        limpio &= r.e == a::BIEN && arch.orden(a::BORRAR, r.asa, 0, 0, nombre).e == a::BIEN;
        arch.cerrar(r.asa);
    }
    let nueva = entradas == 2 && huella == a::NO_EXISTE && teclado.as_deref() == Ok(b"es\n");
    let capas = idioma.as_deref() == Ok(b"ca\n") && tema.as_deref() == Ok(b"claro\n");
    let aparte = cambio == a::BIEN && en_env.as_deref() == Ok(b"zz\n") && en_envarc.as_deref() == Ok(b"ca\n") && en_home.as_deref() == Ok(b"ca\n");
    let bien = nueva && capas && aparte && limpio;
    println!(
        "  [ram] Carla, tras salir y entrar: RAM: nueva ({} entradas, T:huella {}, ENV:Sys/teclado otra vez de fábrica {}); ENV: con su capa encima {}; cambiar ENV: no toca ENVARC: ni Home: {}; limpia {}: {}",
        entradas,
        huella,
        teclado.as_deref() == Ok(b"es\n"),
        capas,
        aparte,
        limpio,
        if bien { "bien" } else { "MAL" }
    );
    bien
}
