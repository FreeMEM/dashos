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
//! Y los avisos de cambio del servicio de archivos (`VIGILAR`, `docs/bitacoras/ajustes-texto.md`): Ana vigila
//! `ENV:Sys` y `ENV:Sys/teclado` y los cambia en ráfaga (un timbre por vigilancia y la cuenta entera), y lo hostil:
//! vigilar lo que no puede leer, una asa mala, una puerta que se cierra, una puerta llena (ningún timbre se pierde),
//! miles de vigilancias, y se va sin soltar las suyas; Carla, después, ve que se soltaron.
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
    let avisos = avisos_ana(arch);
    let hostiles = avisos_hostiles(arch);
    bien && avisos && hostiles
}

/// Avisos de cambio: una puerta más hacia el canal de timbres `escr` (cada `VIGILAR` se lleva la suya).
fn puerta(escr: u32) -> u32 {
    dashos::llave_duplicar(escr, dashos::ESCRIBIR | dashos::TRANSFERIR, 0).max(0) as u32
}

/// Vigila lo que hay en `camino` desde la raíz `raiz`: (estado, número, cuántas tiene, cuántas hay en el servicio).
fn vigilar(arch: &Archivos, raiz: u32, camino: &[u8], escr: u32) -> (i32, u32, u64, u64) {
    let r = arch.orden(a::BUSCAR, raiz, 0, 0, camino);
    if r.e != a::BIEN {
        return (r.e, 0, 0, 0);
    }
    let v = dashos::vigilar::vigilar(ARCHIVOS, arch.lectura, r.asa, puerta(escr), 3_000_000);
    arch.cerrar(r.asa);
    v
}

/// Los timbres que hay en el canal `lect` (esperando hasta `plazo_us` cada uno): cuántos, por número de vigilancia.
fn timbres(lect: u32, plazo_us: u64) -> Vec<u32> {
    let mut v = Vec::new();
    let mut b = [0u8; 32];
    while let (n, _, _) = dashos::canal_leer_plazo(lect, &mut b, &mut [], plazo_us)
        && n >= 0
    {
        if let Some((id, _)) = a::leer_timbre(&b[..n as usize]) {
            v.push(id);
        }
    }
    v
}

/// `AVISOS`: (número, marcas, cambios) de cada vigilancia con cambios.
fn avisos(arch: &Archivos) -> Vec<(u32, u32, u64)> {
    let r = arch.orden(a::AVISOS, 0, 0, 0, b"");
    if r.e != a::BIEN {
        return Vec::new();
    }
    (0..r.a as usize).filter_map(|i| a::leer_aviso(arch.ventana(), i)).collect()
}

/// Ana vigila `ENV:Sys` (el cajón) y `ENV:Sys/teclado` (el fichero) y los cambia: 200 escrituras seguidas son un timbre
/// por vigilancia y una cuenta de 200; tras pedir los avisos, el siguiente cambio vuelve a sonar; crear y borrar en el
/// cajón cuentan para el cajón; borrar lo vigilado avisa con `BORRADO` y la vigilancia acaba.
fn avisos_ana(arch: &Archivos) -> bool {
    let (lect, escr) = dashos::canal_crear();
    // Lo que cuestan 200 escrituras sin nadie vigilando, para comparar.
    let tec = arch.orden(a::BUSCAR, RAM, 0, 0, b"Env/Sys/teclado");
    let t0 = dashos::ahora_us();
    for i in 0..200u64 {
        arch.ventana()[0] = b'a' + (i % 26) as u8;
        arch.orden(a::ESCRIBIR, tec.asa, 0, 1, b"");
    }
    let us_sin = dashos::ahora_us() - t0;
    arch.cerrar(tec.asa);
    let (e_cajon, cajon, _, _) = vigilar(arch, RAM, b"Env/Sys", escr);
    let (e_fichero, fichero, _, _) = vigilar(arch, RAM, b"Env/Sys/teclado", escr);
    let tec = arch.orden(a::BUSCAR, RAM, 0, 0, b"Env/Sys/teclado");
    let t0 = dashos::ahora_us();
    for i in 0..200u64 {
        arch.ventana()[0] = b'a' + (i % 26) as u8;
        arch.orden(a::ESCRIBIR, tec.asa, 0, 1, b"");
    }
    let us_rafaga = dashos::ahora_us() - t0;
    let mut t1 = timbres(lect, 20_000);
    t1.sort();
    let av1 = avisos(arch);
    let rafaga = e_cajon == a::BIEN && e_fichero == a::BIEN && t1 == [cajon, fichero] && av1 == [(cajon, a::AVISO_CAMBIO, 200), (fichero, a::AVISO_CAMBIO, 200)];
    // Pedidos, vuelven a sonar.
    arch.orden(a::ESCRIBIR, tec.asa, 0, 1, b"");
    let t2 = timbres(lect, 20_000).len();
    let av2 = avisos(arch).len();
    // En el cajón: crear y borrar (el fichero no suena).
    let sys = arch.orden(a::BUSCAR, RAM, 0, 0, b"Env/Sys");
    let nuevo = arch.orden(a::CREAR, sys.asa, a::FICHERO as u64, 0, b"nuevo");
    arch.cerrar(nuevo.asa);
    arch.orden(a::BORRAR, sys.asa, 0, 0, b"nuevo");
    let t3 = timbres(lect, 20_000);
    let av3 = avisos(arch);
    // Lo vigilado se borra: avisa con BORRADO, y la vigilancia acaba (dejarla ya no la encuentra).
    arch.poner(sys.asa, b"borrame", b"x");
    let (_, borrame, _, _) = vigilar(arch, RAM, b"Env/Sys/borrame", escr);
    avisos(arch);
    arch.orden(a::BORRAR, sys.asa, 0, 0, b"borrame");
    let av4 = avisos(arch);
    let acabada = arch.orden(a::NO_VIGILAR, borrame, 0, 0, b"").e;
    // Como estaba, y sin vigilar.
    arch.escribir(tec.asa, b"es\n");
    arch.cerrar(tec.asa);
    arch.cerrar(sys.asa);
    let dejar = [cajon, fichero].map(|id| arch.orden(a::NO_VIGILAR, id, 0, 0, b"").e);
    timbres(lect, 20_000);
    avisos(arch);
    dashos::cerrar(lect);
    dashos::cerrar(escr);
    let otra_vez = t2 == 2 && av2 == 2;
    let cajon_bien = t3 == [cajon] && av3 == [(cajon, a::AVISO_CAMBIO, 2)];
    let borrado = av4.iter().any(|&(id, m, _)| id == borrame && m & a::AVISO_BORRADO != 0) && acabada == a::NO_EXISTE;
    let bien = rafaga && otra_vez && cajon_bien && borrado && dejar == [a::BIEN, a::BIEN];
    println!("  [ram] Ana: 200 escrituras de un byte: {} µs sin vigilar, {} µs con dos vigilancias puestas", us_sin, us_rafaga);
    println!(
        "  [ram] Ana: avisos de ENV:Sys y ENV:Sys/teclado: 200 escrituras seguidas, {} timbres y cuentan {:?}; pedidos, uno más suena otra vez {}; crear y borrar en el cajón {:?}; borrar lo vigilado {:?} y acaba {}: {}",
        t1.len(),
        av1.iter().map(|x| x.2).collect::<Vec<_>>(),
        otra_vez,
        av3.iter().map(|x| x.2).collect::<Vec<_>>(),
        av4.iter().map(|x| x.1).collect::<Vec<_>>(),
        acabada,
        if bien { "bien" } else { "MAL" }
    );
    bien
}

/// Lo hostil de los avisos: vigilar lo que no se puede leer (`SYS:DeCarla`, de Carla y 0700) o por una asa mala; una
/// puerta cuyo canal se cierra (la vigilancia se quita sola); 63 vigilancias con la misma puerta y nadie leyendo (el
/// canal se llena: los timbres esperan y llegan todos); y miles pedidas: 64 como mucho, y el servicio sigue. Se va con
/// las 64 puestas: Carla, después, ve que se soltaron.
fn avisos_hostiles(arch: &Archivos) -> bool {
    let (lect, escr) = dashos::canal_crear();
    let ajeno = vigilar(arch, s::RAIZ_SYS as u32, b"DeCarla", escr).0;
    let mala = dashos::vigilar::vigilar(ARCHIVOS, arch.lectura, 9999, puerta(escr), 3_000_000).0;
    let sin_puerta = arch.orden(a::VIGILAR, RAM, 0, 0, b"").e;
    // Una puerta que se cierra: al primer cambio, el servicio ve que nadie lee y la quita.
    let (l2, e2) = dashos::canal_crear();
    let (_, cerrada, _, _) = vigilar(arch, RAM, b"Env/Sys/teclado", e2);
    dashos::cerrar(l2);
    dashos::cerrar(e2);
    let tec = arch.orden(a::BUSCAR, RAM, 0, 0, b"Env/Sys/teclado");
    arch.orden(a::ESCRIBIR, tec.asa, 0, 0, b"");
    arch.escribir(tec.asa, b"es\n");
    let quitada = arch.orden(a::NO_VIGILAR, cerrada, 0, 0, b"").e;
    // La puerta llena: 63 vigilancias del mismo fichero, un cambio, y el canal sin leer.
    let mut ids = Vec::new();
    for _ in 0..63 {
        let (e, id, _, _) = vigilar(arch, RAM, b"Env/Sys/teclado", escr);
        if e == a::BIEN {
            ids.push(id);
        }
    }
    arch.escribir(tec.asa, b"es\n");
    dashos::dormir_us(300_000);
    // Se leen despacio: los que no cupieron llegan por el reintento.
    let mut llegados = timbres(lect, 300_000);
    llegados.sort();
    llegados.dedup();
    let av = avisos(arch);
    let todos = ids.len() == 63 && llegados == ids && av.len() == 63 && av.iter().all(|x| x.1 == a::AVISO_CAMBIO && x.2 == 2);
    // Miles: con 63 puestas, una más y ya no hay sitio; el servicio sigue atendiendo.
    let t0 = dashos::ahora_us();
    let (mut si, mut no, mut otro) = (0, 0, 0);
    for _ in 0..5000 {
        match dashos::vigilar::vigilar(ARCHIVOS, arch.lectura, tec.asa, puerta(escr), 3_000_000).0 {
            a::BIEN => si += 1,
            a::SIN_SITIO => no += 1,
            _ => otro += 1,
        }
    }
    let ms = (dashos::ahora_us() - t0) / 1000;
    let sigue = arch.esta(RAM, b"Env/Sys/teclado");
    arch.cerrar(tec.asa);
    timbres(lect, 10_000);
    let bien = ajeno == a::PROTEGIDO && mala == a::ASA_MALA && sin_puerta == a::MAL_FORMADO && quitada == a::NO_EXISTE && todos && (si, no, otro) == (1, 4999, 0) && sigue == a::BIEN;
    println!("  [ram] Ana: 5000 VIGILAR (4999 sin sitio): {} ms", ms);
    println!(
        "  [ram] Ana: avisos hostiles: SYS:DeCarla {}, asa mala {}, sin puerta {}; la puerta cerrada se quita {}; 63 con la puerta llena, llegan {} timbres y {} avisos de 2; 5000 pedidas: {} sí, {} sin sitio, {} otro; el servicio sigue {}: {}",
        ajeno,
        mala,
        sin_puerta,
        quitada,
        llegados.len(),
        av.len(),
        si,
        no,
        otro,
        sigue,
        if bien { "bien" } else { "MAL" }
    );
    // Se va con las 64 puestas y su canal abierto (`lect` no se cierra): al acabar el programa, el servicio las suelta
    // (lo mira Carla).
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
    // Ana se fue con 64 vigilancias puestas: el servicio las soltó al acabar ella (las que haya son de otros: la barra,
    // el centro).
    let (lect, escr) = dashos::canal_crear();
    let (e_vig, id, suyas, total) = vigilar(arch, RAM, b"Env", escr);
    let dejada = arch.orden(a::NO_VIGILAR, id, 0, 0, b"").e;
    dashos::cerrar(lect);
    dashos::cerrar(escr);
    let soltadas = e_vig == a::BIEN && suyas == 1 && total < 32 && dejada == a::BIEN;
    println!("  [ram] Carla: las 64 vigilancias de Ana se soltaron al acabar ella: {} en el servicio con la de Carla: {}", total, if soltadas { "bien" } else { "MAL" });
    // Ajustes en texto, hostil: lo de la máquina (la zona) y la capa de fábrica de `ENVARC:` no se escriben (son de
    // `SYS:`, que le llega solo para leer); lo de Ana, ni se alcanza (arriba).
    let zona = arch.orden(a::BUSCAR, s::RAIZ_SYS as u32, 0, 0, b"Prefs/zona");
    let e_zona = if zona.e == a::BIEN { arch.ventana()[..3].copy_from_slice(b"UTC"); let e = arch.orden(a::ESCRIBIR, zona.asa, 0, 3, b"").e; arch.cerrar(zona.asa); e } else { zona.e };
    let fabrica_sys = arch.orden(a::BUSCAR, s::RAIZ_SYS as u32, 0, 0, b"Prefs/Env-Archive/Sys");
    let e_fabrica = if fabrica_sys.e == a::BIEN { let r = arch.orden(a::CREAR, fabrica_sys.asa, a::FICHERO as u64, 0, b"locale"); arch.cerrar(fabrica_sys.asa); r.e } else { fabrica_sys.e };
    // Y guardar `ENV:` en `ENVARC:` por `sesiones` (`GUARDAR_ENV`) es solo del centro de control: un programa cualquiera
    // no puede, ni de su sesión ni de otra (el mensaje no nombra la sesión), ni con un nombre que sale del cajón; y la
    // orden de `archivos` que lo hace es de `sesiones` (con la llave de un programa no existe).
    let guardar_env = |nombre: &[u8]| -> i32 {
        let (lectura, suyo) = dashos::canal_crear();
        let mut m = [0u8; 8 + 64];
        m[0..4].copy_from_slice(&s::GUARDAR_ENV.to_le_bytes());
        m[4..8].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        m[8..8 + nombre.len()].copy_from_slice(nombre);
        let mut r = [0u8; 4];
        let n = if dashos::canal_escribir(SESIONES, &m[..8 + nombre.len()], &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 3_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }
    };
    let pedidos = [guardar_env(b"Sys/locale"), guardar_env(b"../../Prefs/Red/hora")];
    let mut cab = [0u8; 64];
    cab[0..4].copy_from_slice(&1000u32.to_le_bytes());
    cab[16] = 3;
    cab[17] = 10;
    cab[32..35].copy_from_slice(b"Ana");
    cab[35..45].copy_from_slice(b"Sys/locale");
    let directo = arch.orden(a::RAM_GUARDAR, 1000, 1000, 1000, &cab).e;
    let maquina = e_zona == a::PROTEGIDO && e_fabrica == a::PROTEGIDO && pedidos == [s::PROTEGIDO, s::PROTEGIDO] && directo == a::MAL_FORMADO;
    println!("  [ram] Carla: guardar ENV: en ENVARC: por sesiones {:?}; la orden de archivos con su llave {}", pedidos, directo);
    println!("  [ram] Carla: escribir la zona de la máquina {} y en ENVARC: de fábrica {}: {}", e_zona, e_fabrica, if maquina { "bien" } else { "MAL" });
    let soltadas = soltadas && maquina;
    let bien = aislada && fabrica && cambio == a::BIEN && e_huella == a::BIEN && capa == Ok(()) && envarc && soltadas;
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
    // Solo lo que puso esta prueba: en `Prefs/Env-Archive/Sys` está también su `locale.prefs` (idiomas), que se queda.
    for (cajon, nombre) in [(&b"Prefs/Env-Archive/Sys"[..], &b"idioma"[..]), (b"Prefs/Env-Archive/Escritorio", b"tema"), (b"Prefs/Env-Archive", b"Escritorio"), (b"", b"ram.fase")] {
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
