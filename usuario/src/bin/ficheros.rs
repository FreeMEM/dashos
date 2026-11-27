//! Prueba del protocolo de ficheros (FS3, FS4): un cliente del canal "archivos" (la llave 1, con su marca de
//! `inicio`). Abre su sesión, lista la raíz, lee `LEEME.txt` y `C/grande.txt` entero por trozos de la ventana, pide
//! los datos de un fichero y del volumen, y comprueba los rechazos: lo que no existe, una asa cerrada, leer un cajón y
//! listar un fichero. Después escribe: crea un cajón y un fichero de 200 KB, lo relee, lo recorta, lo mueve, lo borra
//! abierto y deja `Escrito/hola.txt` para que `tools/disco_archivos.py --comprobar` lo vea en Linux. Por último, FS6
//! y FS7: buscar sin mirar mayúsculas, el comentario y los bits del Amiga, que dejan `hola.txt` sin escribir ni
//! borrar mientras lo pidan. Y FS8: el servicio comprueba el disco entero y no encuentra nada. FS9: tomar una
//! instantánea, volver a ella y descartarla. FS10: volúmenes; el cifrado "Ana" (hecho en Linux) no se lee cerrado, se
//! abre con su contraseña y no con otra, y uno sin cifrar se hace y se quita.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::archivos::*;

dashos::programa!(principal);

const VA: usize = 0x40_0000_0000;
// Bits del Amiga (`AmigaBits` de DashFS).
const ARCHIVO: u64 = 1;
const GUION: u64 = 4;
const NO_BORRAR: u64 = 16;

#[derive(Clone, Copy)]
struct R {
    estado: i32,
    asa: u32,
    a: u64,
    b: u64,
    c: u64,
}

/// Si ya se dijo que el servicio no admitía una petición (una vez basta).
static AVISADO_LLENO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// Si ya se dijo que se espera a que otro suelte el disco (una vez basta).
static AVISADO_SUJETO: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// A15: lo que se deja libre, además del 1/16 del disco con el que el servicio suelta lo sujetado, para lo que escriben
/// los demás programas mientras este sujeta el disco (la prueba "A15 sujetar").
const RESERVA: u64 = 256 * 1024;
/// A15: lo mismo mientras lo sujeta otro (ver `Cliente::dejar_sitio`), que puede durar mucho más: lo que gastan los
/// demás va con el tiempo (cada guardado de cualquiera reescribe unos bloques de cajones, y con el disco sujeto los
/// viejos no se reusan). Medido (rama a15-carrera): con el PC cargado, la foto del comprobador duró 29 s y, con este
/// programa ya parado, los demás gastaron 104 bloques (416 KB); sin carga, 46 en 3 s. 1 MB es el doble y algo más. En
/// el disco de pruebas (unos 1,5 MB libres) quiere decir que mientras el comprobador sujeta, aquí no se escribe.
const RESERVA_AJENA: u64 = 1024 * 1024;
/// A15: lo más que se espera a que otro suelte el disco (el comprobador, con el PC cargado, ha tardado 31 s).
const ESPERA_SUJETO_US: u64 = 120_000_000;

struct Cliente {
    lectura: u32,
    ventana: usize,
    /// A15: este programa tiene el disco sujeto (`SUJETAR` le dio 0 y aún no ha pedido `SOLTAR`).
    sujeto: core::cell::Cell<bool>,
}

impl Cliente {
    fn leer_respuesta(&self, llaves: &mut [u32]) -> Option<(R, usize)> {
        let mut b = [0u8; 32];
        let (n, k) = dashos::canal_leer(self.lectura, &mut b, llaves);
        if n != 32 {
            return None;
        }
        let u32_en = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let u64_en = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        Some((R { estado: u32_en(0) as i32, asa: u32_en(4), a: u64_en(8), b: u64_en(16), c: u64_en(24) }, k))
    }

    fn pedir(&self, orden: u32, asa: u32, a: u64, b: u64, nombre: &str) -> R {
        if orden == ESCRIBIR && !self.sujeto.get() {
            self.dejar_sitio(b);
        }
        let r = self.pedir_ya(orden, asa, a, b, nombre);
        match orden {
            SUJETAR if r.estado == 0 => self.sujeto.set(true),
            SOLTAR => self.sujeto.set(false),
            _ => {}
        }
        r
    }

    /// A15: el comprobador (otro proceso) recorre una foto del disco sujeta mientras este programa escribe. Mientras
    /// está sujeto, lo que se borra no se reusa; si el sitio libre baja de 1/16 del disco, el servicio lo suelta y la
    /// prueba del comprobador se queda a medias. Así que, antes de escribir `n` bytes: si no caben dejando ese 1/16 y
    /// `RESERVA_AJENA`, y es porque otro tiene el disco sujeto (`INFO` lo dice), se espera a que lo suelte. Sin nadie
    /// que lo sujete, se escribe (lo borrado se reusa). Así no importa en qué prueba de este programa empiece a sujetar el otro, ni
    /// cuánto ocupe el disco: lo que se escribe mientras tanto se para antes de quitarle el sitio.
    fn dejar_sitio(&self, n: u64) {
        let desde = dashos::ahora_us();
        while dashos::ahora_us() - desde < ESPERA_SUJETO_US {
            let info = self.pedir_ya(INFO, 0, 0, 0, "");
            if info.estado != 0 || info.asa == 0 || info.b >= n + info.a / 16 + RESERVA_AJENA {
                return;
            }
            if !AVISADO_SUJETO.swap(true, core::sync::atomic::Ordering::Relaxed) {
                println!("  [ficheros] A15: otro tiene el disco sujeto y quedan {} KB libres: se espera a que lo suelte para escribir", info.b / 1024);
            }
            dashos::dormir_us(10_000);
        }
        println!("  [ficheros] A15: otro tiene el disco sujeto más de {} s: se escribe igual", ESPERA_SUJETO_US / 1_000_000);
    }

    fn pedir_ya(&self, orden: u32, asa: u32, a: u64, b: u64, nombre: &str) -> R {
        let mut p = [0u8; 256];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&a.to_le_bytes());
        p[16..24].copy_from_slice(&b.to_le_bytes());
        p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        p[32..32 + nombre.len()].copy_from_slice(nombre.as_bytes());
        let malo = R { estado: i32::MIN, asa: 0, a: 0, b: 0, c: 0 };
        // Con el servicio atareado (el PC cargado), su cola puede estar llena: se reintenta un rato en vez de dar la
        // petición por fallida (si no, la siguiente respuesta que llega es de otra y todo se desacompasa).
        let mut intentos = 0;
        loop {
            let e = dashos::canal_escribir(1, &p[..32 + nombre.len()], &[]);
            if e == 0 {
                break;
            }
            if e == dashos::E_CERRADO || intentos >= 2000 {
                return malo;
            }
            if intentos == 0 && !AVISADO_LLENO.swap(true, core::sync::atomic::Ordering::Relaxed) {
                println!("  [ficheros] el servicio de archivos no admite la petición ({}): se reintenta", e);
            }
            intentos += 1;
            dashos::dormir_us(1_000);
        }
        self.leer_respuesta(&mut []).map_or(malo, |r| r.0)
    }

    /// Abre un volumen cifrado: la contraseña va por la ventana, no en el mensaje (A13); el servicio la borra de ahí.
    fn abrir_volumen(&self, nombre: &str, clave: &str) -> R {
        self.ventana_mut()[..clave.len()].copy_from_slice(clave.as_bytes());
        let r = self.pedir(ABRIR_VOLUMEN, 0, 0, clave.len() as u64, nombre);
        if self.ventana()[..clave.len()].iter().any(|&b| b != 0) {
            println!("  [ficheros] la contraseña sigue en la ventana");
            return R { estado: i32::MIN, asa: 0, a: 0, b: 0, c: 0 };
        }
        r
    }

    fn ventana(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ventana as *const u8, 64 * 1024) }
    }

    fn ventana_mut(&self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ventana as *mut u8, 64 * 1024) }
    }
}

/// El byte `i` de los datos de prueba de 200 KB.
fn dato(i: u64) -> u8 {
    (i.wrapping_mul(2654435761) >> 13) as u8
}

const VA_PRESTADOS: usize = 0x41_0000_0000;

/// Escribe `tam` bytes de `dato(i + sal)` en `nombre` del cajón `dir`; su asa.
fn hacer(c: &Cliente, dir: u32, nombre: &str, tam: u64, sal: u64) -> Option<u32> {
    let f = c.pedir(CREAR, dir, 1, 0, nombre);
    if f.estado != 0 {
        return None;
    }
    let mut off = 0;
    while off < tam {
        let n = (tam - off).min(64 * 1024);
        for (i, b) in c.ventana_mut()[..n as usize].iter_mut().enumerate() {
            *b = dato(off + i as u64 + sal);
        }
        if c.pedir(ESCRIBIR, f.asa, off, n, "").a != n {
            return None;
        }
        off += n;
    }
    Some(f.asa)
}

/// Lee `asa` entero con lo prestado y lo compara con `dato(i + sal)` (con `cambio` puesto encima).
fn igual(pr: &mut dashos::prestados::Prestados, lectura: u32, asa: u32, tam: u64, sal: u64, cambio: (u64, &[u8])) -> bool {
    let mut b = [0u8; 4096];
    let mut off = 0;
    while off < tam {
        let Some(n) = pr.leer(1, lectura, asa, off, &mut b[..]) else { return false };
        if n == 0 {
            return false;
        }
        for (i, &x) in b[..n].iter().enumerate() {
            let j = off + i as u64;
            let quiero = if j >= cambio.0 && j < cambio.0 + cambio.1.len() as u64 { cambio.1[(j - cambio.0) as usize] } else { dato(j + sal) };
            if x != quiero {
                println!("  [ficheros] A7: en {} hay {} y no {}", j, x, quiero);
                return false;
            }
        }
        off += n as u64;
    }
    off == tam
}

fn prestadas(c: &Cliente, dir: u32) -> bool {
    const TAM: u64 = 600 * 1024;
    let Some(asa) = hacer(c, dir, "presta.bin", TAM, 0) else { return false };
    let mut pr = dashos::prestados::Prestados::nuevo(VA_PRESTADOS);
    let mut partes = [false; 6];
    partes[0] = igual(&mut pr, c.lectura, asa, TAM, 0, (0, &[])) && igual(&mut pr, c.lectura, asa, TAM, 0, (0, &[])) && pr.viajes == 1;
    // Escribir por el protocolo: lo prestado ya lo tiene, sin otro viaje.
    c.ventana_mut()[..10].copy_from_slice(b"0123456789");
    partes[1] = c.pedir(ESCRIBIR, asa, 530_000, 10, "").a == 10 && igual(&mut pr, c.lectura, asa, TAM, 0, (530_000, b"0123456789")) && pr.viajes == 1;
    // Recortar a 540.000: lo de después, ceros (aunque se lea con el tamaño de antes).
    let mut b = [1u8; 64];
    partes[2] = c.pedir(TAMANO, asa, 540_000, 0, "").estado == 0
        && pr.leer(1, c.lectura, asa, 600_000, &mut b).is_some()
        && b.iter().all(|&x| x == 0);
    // Cerrar, borrar y hacer otro: lo nuevo, no lo de antes.
    pr.cerrar_asa(asa);
    let otro = c.pedir(CERRAR, asa, 0, 0, "").estado == 0 && c.pedir(BORRAR, dir, 0, 0, "presta.bin").estado == 0;
    let nuevo = if otro { hacer(c, dir, "presta2.bin", TAM, 7) } else { None };
    partes[3] = nuevo.is_some_and(|n| igual(&mut pr, c.lectura, n, TAM, 7, (0, &[])));
    if let Some(n) = nuevo {
        pr.cerrar_asa(n);
        c.pedir(CERRAR, n, 0, 0, "");
        c.pedir(BORRAR, dir, 0, 0, "presta2.bin");
    }
    // Uno pequeño no se presta: `None`, y se lee con LEER.
    let h = c.pedir(BUSCAR, dir, 0, 0, "hola.txt");
    partes[4] = h.estado == 0 && pr.leer(1, c.lectura, h.asa, 0, &mut b).is_none();
    c.pedir(CERRAR, h.asa, 0, 0, "");
    // Un cajón tampoco.
    partes[5] = pr.leer(1, c.lectura, dir, 0, &mut b).is_none();
    if !partes.iter().all(|&p| p) {
        println!("  [ficheros] A7 por partes: {:?}", partes);
    }
    partes.iter().all(|&p| p)
}

fn principal() -> u64 {
    let (lectura, vuelta) = dashos::canal_crear();
    let mut c = Cliente { lectura, ventana: 0, sujeto: core::cell::Cell::new(false) };
    // La sesión: el extremo de escritura del canal de respuesta va con el mensaje.
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&SESION.to_le_bytes());
    if dashos::canal_escribir(1, &p, &[vuelta]) != 0 {
        println!("  [ficheros] sin servicio de archivos");
        return 1;
    }
    let mut ll = [0u32; 1];
    let Some((r, 1)) = c.leer_respuesta(&mut ll).filter(|(r, _)| r.estado == 0) else {
        println!("  [ficheros] sin sesión");
        return 2;
    };
    if r.a != 64 * 1024 || dashos::memoria_proyectar(ll[0], VA, true) != 0 {
        println!("  [ficheros] no se proyecta la ventana");
        return 3;
    }
    c.ventana = VA;
    let si = |b: bool| if b { "bien" } else { "MAL" };

    // Listar la raíz: tamaño, fecha, modo, tipo, largo del nombre, bits del Amiga, largo del comentario, nombre y
    // comentario, de 8 en 8 bytes.
    let r = c.pedir(LISTAR, 0, 0, 0, "");
    // Paso 2 de las asignaciones: la raíz es `SYS:`, con sus cajones (21 entradas).
    let mut nombres: [(&str, u8); 32] = [("", 0); 32];
    let v = c.ventana();
    let mut off = 0;
    for e in nombres.iter_mut().take(r.a as usize) {
        let largo = v[off + 19] as usize;
        *e = (core::str::from_utf8(&v[off + 24..off + 24 + largo]).unwrap_or("?"), v[off + 18]);
        off += (24 + largo + v[off + 22] as usize).next_multiple_of(8);
    }
    let lista = &mut nombres[..(r.a as usize).min(32)];
    lista.sort_unstable();
    let mut texto = [0u8; 512];
    let mut t = 0;
    for (n, tipo) in lista.iter() {
        for b in n.bytes().chain(if *tipo == 2 { "/ " } else { " " }.bytes()) {
            texto[t] = b;
            t += 1;
        }
    }
    println!("  [ficheros] raíz ({} entradas, fin {}): {}", r.a, si(r.b == u64::MAX), core::str::from_utf8(&texto[..t]).unwrap().trim_end());

    // LEEME.txt.
    let f = c.pedir(BUSCAR, 0, 0, 0, "LEEME.txt");
    let l = c.pedir(LEER, f.asa, 0, 4096, "");
    println!("  [ficheros] LEEME.txt ({} bytes): {:?}", f.a, core::str::from_utf8(&c.ventana()[..l.a as usize]).unwrap_or("?").trim_end());
    // A3: lo mismo en un viaje (la asa de arriba sigue abierta: la usan los rechazos), desde el byte 7, y lo que no existe.
    let ln = c.pedir(LEER_NOMBRE, 0, 7, 4096, "LEEME.txt");
    let a3 = ln.estado == 0 && ln.a == 12 && ln.c == 19 && ln.asa == 0 && &c.ventana()[..12] == b"lee RedoxFS\n"
        && c.pedir(LEER_NOMBRE, 0, 0, 4096, "no-existe").estado == -2
        && c.pedir(LEER_NOMBRE, 0, 0, 4096, "C").estado == -4;
    println!("  [ficheros] A3 leer por nombre: {}", si(a3));

    // C/grande.txt por trozos de 64 KB: las líneas "línea 0", "línea 1"...
    let g = c.pedir(BUSCAR, 0, 0, 0, "C/grande.txt");
    let mut esperado = [0u8; 16];
    let (mut linea, mut en_linea) = (0u32, 0usize);
    let poner = |linea: u32, buf: &mut [u8; 16]| -> usize {
        use core::fmt::Write;
        struct B<'a>(&'a mut [u8; 16], usize);
        impl Write for B<'_> {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                self.0[self.1..self.1 + s.len()].copy_from_slice(s.as_bytes());
                self.1 += s.len();
                Ok(())
            }
        }
        let mut b = B(buf, 0);
        let _ = writeln!(b, "línea {}", linea);
        b.1
    };
    let mut largo_linea = poner(linea, &mut esperado);
    let (mut total, mut bien) = (0u64, g.estado == 0);
    while bien && total < g.a {
        let r = c.pedir(LEER, g.asa, total, 64 * 1024, "");
        if r.estado != 0 || r.a == 0 {
            bien = false;
            break;
        }
        for &b in &c.ventana()[..r.a as usize] {
            if b != esperado[en_linea] {
                bien = false;
                break;
            }
            en_linea += 1;
            if en_linea == largo_linea {
                linea += 1;
                en_linea = 0;
                largo_linea = poner(linea, &mut esperado);
            }
        }
        total += r.a;
    }
    println!("  [ficheros] C/grande.txt: {} bytes, {} líneas: {}", total, linea, si(bien && total == g.a));

    // Datos de un fichero (tipo y modo) y del volumen.
    let d = c.pedir(DATOS, g.asa, 0, 0, "");
    let info = c.pedir(INFO, 0, 0, 0, "");
    println!("  [ficheros] datos: {}; volumen: {} KB, {} KB libres", si(d.estado == 0 && d.a == g.a && d.c >> 16 == 1), info.a / 1024, info.b / 1024);

    // Rechazos.
    let cajon = c.pedir(BUSCAR, 0, 0, 0, "C");
    let rechazos = c.pedir(BUSCAR, 0, 0, 0, "no/existe").estado == -2
        && c.pedir(BUSCAR, f.asa, 0, 0, "x").estado == -3
        && c.pedir(LEER, cajon.asa, 0, 10, "").estado == -4
        && c.pedir(LISTAR, f.asa, 0, 0, "").estado == -3
        && c.pedir(CERRAR, f.asa, 0, 0, "").estado == 0
        && c.pedir(LEER, f.asa, 0, 10, "").estado == -5
        && c.pedir(BUSCAR, 0, 0, 0, "C//x").estado == -1
        && c.pedir(99, 0, 0, 0, "").estado == -1;
    // Una búsqueda desde un cajón abierto.
    let desde = c.pedir(BUSCAR, cajon.asa, 0, 0, "grande.txt");
    println!("  [ficheros] rechazos: {}; desde un cajón: {}", si(rechazos), si(desde.estado == 0 && desde.a == g.a));

    // FS4: escribir. Un cajón y un fichero de 200 KB, por trozos de 64 KB. Lo libre se mide tras esperar, si hace
    // falta, a que el comprobador suelte el disco (al soltarlo vuelve lo que retenía: si no, lo libre podría subir).
    const TAM: u64 = 200 * 1024;
    c.dejar_sitio(TAM);
    let libre_antes = c.pedir(INFO, 0, 0, 0, "").b;
    let dir = c.pedir(CREAR, 0, 2, 0, "Escrito");
    let f = c.pedir(CREAR, dir.asa, 1, 0, "datos.bin");
    let mut escrito = dir.estado == 0 && f.estado == 0;
    let mut off = 0;
    while escrito && off < TAM {
        let n = (TAM - off).min(64 * 1024);
        for (i, b) in c.ventana_mut()[..n as usize].iter_mut().enumerate() {
            *b = dato(off + i as u64);
        }
        let r = c.pedir(ESCRIBIR, f.asa, off, n, "");
        escrito = r.estado == 0 && r.a == n && r.b == off + n;
        off += n;
    }
    let mut releido = escrito;
    off = 0;
    while releido && off < TAM {
        let r = c.pedir(LEER, f.asa, off, 64 * 1024, "");
        releido = r.estado == 0 && r.a > 0 && c.ventana()[..r.a as usize].iter().enumerate().all(|(i, &b)| b == dato(off + i as u64));
        off += r.a;
    }
    let ocupa = c.pedir(INFO, 0, 0, 0, "").b < libre_antes;
    println!("  [ficheros] escribir 200 KB: {}; releer: {}; ocupa sitio: {}", si(escrito), si(releido && off == TAM), si(ocupa));

    // Recortar, renombrar, mover a la raíz y borrar abierto: sigue leyéndose hasta cerrarlo.
    let recortado = c.pedir(TAMANO, f.asa, 1000, 0, "").estado == 0 && c.pedir(DATOS, f.asa, 0, 0, "").a == 1000;
    let renombrado = c.pedir(RENOMBRAR, dir.asa, 0, dir.asa as u64, "datos.bin\0otro.bin").estado == 0
        && c.pedir(BUSCAR, dir.asa, 0, 0, "datos.bin").estado == -2
        && c.pedir(RENOMBRAR, dir.asa, 0, 0, "otro.bin\0movido.bin").estado == 0
        && c.pedir(BUSCAR, 0, 0, 0, "movido.bin").estado == 0;
    let borrado = c.pedir(BORRAR, 0, 0, 0, "movido.bin").estado == 0
        && c.pedir(BUSCAR, 0, 0, 0, "movido.bin").estado == -2
        && c.pedir(LEER, f.asa, 0, 1000, "").a == 1000
        && c.ventana()[..1000].iter().enumerate().all(|(i, &b)| b == dato(i as u64))
        && c.pedir(CERRAR, f.asa, 0, 0, "").estado == 0;
    println!("  [ficheros] recortar: {}; renombrar y mover: {}; borrar abierto: {}", si(recortado), si(renombrado), si(borrado));

    // Rechazos al escribir.
    let h = c.pedir(CREAR, dir.asa, 1, 0, "hola.txt");
    c.ventana_mut()[..20].copy_from_slice(b"hola desde DashOS!\n\0");
    let hola = h.estado == 0 && c.pedir(ESCRIBIR, h.asa, 0, 19, "").a == 19;
    let sub = c.pedir(CREAR, dir.asa, 2, 0, "Sub");
    let rechazos = c.pedir(CREAR, dir.asa, 1, 0, "hola.txt").estado == -9
        && c.pedir(CREAR, 0, 1, 0, "a/b").estado == -1
        && c.pedir(BORRAR, 0, 0, 0, "Escrito").estado == -10
        && c.pedir(ESCRIBIR, dir.asa, 0, 1, "").estado == -4
        && c.pedir(RENOMBRAR, 0, 0, sub.asa as u64, "Escrito\0x").estado == -1
        && c.pedir(RENOMBRAR, dir.asa, 0, dir.asa as u64, "Sub\0hola.txt").estado == -9
        && c.pedir(BORRAR, dir.asa, 0, 0, "Sub").estado == 0
        && c.pedir(VACIAR, 0, 0, 0, "").estado == 0;
    println!("  [ficheros] hola.txt: {}; rechazos al escribir: {}", si(hola), si(rechazos));

    // A7: páginas prestadas. Un fichero de 600 KB (dos trozos) se lee entero dos veces con un solo viaje; lo que se
    // escribe después se ve sin viajar; al recortar, lo de después son ceros; un fichero nuevo que pueda tener el número
    // de uno borrado no enseña lo viejo; y uno pequeño no se presta.
    println!("  [ficheros] A7 páginas prestadas: {}", si(prestadas(&c, dir.asa)));

    // FS6: la raíz no distingue mayúsculas.
    let mayus = c.pedir(BUSCAR, 0, 0, 0, "leeme.TXT");
    let mayusculas = mayus.estado == 0 && mayus.a == 19 && c.pedir(CERRAR, mayus.asa, 0, 0, "").estado == 0;
    let otra = c.pedir(BUSCAR, 0, 0, 0, "ESCRITO/HOLA.TXT");
    let mayusculas = mayusculas && otra.estado == 0 && c.pedir(CERRAR, otra.asa, 0, 0, "").estado == 0;

    // FS7: el comentario.
    const NOTA: &str = "Saludo de DashOS, ñandú";
    let r = (c.pedir(COMENTAR, h.asa, 0, 0, NOTA), c.pedir(COMENTARIO, h.asa, 0, 0, ""));
    let comentario = r.0.estado == 0 && r.1.estado == 0 && &c.ventana()[..r.1.a as usize] == NOTA.as_bytes();
    let r = c.pedir(LISTAR, dir.asa, 0, 0, "");
    let v = c.ventana();
    let mut en_lista = false;
    let mut off = 0;
    for _ in 0..r.a {
        let (largo, largo_c) = (v[off + 19] as usize, v[off + 22] as usize);
        let fin = off + 24 + largo;
        if &v[off + 24..fin] == b"hola.txt" {
            en_lista = &v[fin..fin + largo_c] == NOTA.as_bytes();
        }
        off += (24 + largo + largo_c).next_multiple_of(8);
    }

    // FS7: los bits. Sin la W (modo 0444) no se escribe ni se recorta; con "no borrar", no se borra; escribir quita
    // el bit de archivado.
    let modo_y_bits = |c: &Cliente| {
        let d = c.pedir(DATOS, h.asa, 0, 0, "");
        (d.c & 0o7777, d.c >> 32)
    };
    let mut bits = c.pedir(PROTEGER, h.asa, ARCHIVO | GUION, 0o444, "").estado == 0
        && modo_y_bits(&c) == (0o444, ARCHIVO | GUION)
        && c.pedir(ESCRIBIR, h.asa, 0, 1, "").estado == -12
        && c.pedir(TAMANO, h.asa, 0, 0, "").estado == -12
        && c.pedir(PROTEGER, h.asa, ARCHIVO | NO_BORRAR, 0o644, "").estado == 0
        && c.pedir(BORRAR, dir.asa, 0, 0, "hola.txt").estado == -12;
    c.ventana_mut()[..19].copy_from_slice(b"hola desde DashOS!\n");
    bits = bits
        && c.pedir(ESCRIBIR, h.asa, 0, 19, "").a == 19
        && modo_y_bits(&c) == (0o644, NO_BORRAR)
        && c.pedir(PROTEGER, h.asa, GUION, 0o644, "").estado == 0
        && c.pedir(PROTEGER, h.asa, 1 << 20, 0o644, "").estado == -1;
    // La fecha de modificación (FECHAR): se pone y se lee con DATOS; nanosegundos de más, una asa que no es o una fecha
    // fuera de rango, no.
    // Al acabar se le devuelve su fecha: la comprobación del disco tras la prueba mira que lo escrito sea de hoy.
    let antes = c.pedir(DATOS, h.asa, 0, 0, "").b;
    let fecha = c.pedir(FECHAR, h.asa, 1_700_000_000, 5, "").estado == 0
        && c.pedir(DATOS, h.asa, 0, 0, "").b == 1_700_000_000
        && c.pedir(FECHAR, h.asa, 1_700_000_000, 1_000_000_000, "").estado == -1
        && c.pedir(FECHAR, h.asa, u64::MAX, 0, "").estado == -1
        && c.pedir(FECHAR, 999, 1_700_000_000, 0, "").estado == -5
        && c.pedir(DATOS, h.asa, 0, 0, "").b == 1_700_000_000
        && c.pedir(FECHAR, h.asa, antes, 0, "").estado == 0
        && c.pedir(DATOS, h.asa, 0, 0, "").b == antes;
    println!("  [ficheros] fecha de modificación: {}", si(fecha));
    println!(
        "  [ficheros] FS6 mayúsculas: {}; FS7 comentario: {}, en la lista: {}; bits: {}",
        si(mayusculas),
        si(comentario),
        si(en_lista),
        si(bits)
    );

    // A11: lo recién borrado espera en cuarentena hasta guardar. COMPROBAR ya no guarda antes: lo cuenta como libre
    // pendiente, no como perdido.
    let escrito = c.pedir(BUSCAR, 0, 0, 0, "Escrito");
    if let Some(a) = hacer(&c, escrito.asa, "a11.bin", 300 * 1024, 3) {
        c.pedir(CERRAR, a, 0, 0, "");
        c.pedir(BORRAR, escrito.asa, 0, 0, "a11.bin");
    }
    c.pedir(CERRAR, escrito.asa, 0, 0, "");
    // A14: lo creado lleva la hora del sistema (la de antes de pedirlo, o hasta 2 s después); sin hora, 0.
    let h = dashos::hora_ns() / 1_000_000_000;
    let escrito = c.pedir(BUSCAR, 0, 0, 0, "Escrito");
    let fecha = match hacer(&c, escrito.asa, "fecha.bin", 100, 0) {
        Some(a) => {
            let d = c.pedir(DATOS, a, 0, 0, "");
            c.pedir(CERRAR, a, 0, 0, "");
            c.pedir(BORRAR, escrito.asa, 0, 0, "fecha.bin");
            d.estado == 0 && if h == 0 { d.b == 0 } else { d.b >= h && d.b <= h + 2 }
        }
        None => false,
    };
    c.pedir(CERRAR, escrito.asa, 0, 0, "");
    println!("  [ficheros] A14 fechas: {}{}", si(fecha), if h == 0 { " (sin hora)" } else { "" });
    // A14, lectura anticipada: C/frio.bin (1 MB de xorshift, de la imagen: frío de verdad), leído a trozos de 64 KB
    // mientras se escribe y se guarda otro fichero entre trozo y trozo (cada guardado recoge lo leído por adelantado y
    // lo da por no válido). Tiene que salir entero y bien.
    let frio = c.pedir(BUSCAR, 0, 0, 0, "C/frio.bin");
    let escrito = c.pedir(BUSCAR, 0, 0, 0, "Escrito");
    let otro = c.pedir(CREAR, escrito.asa, 1, 0, "entre.bin");
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut adelanto = frio.estado == 0 && otro.estado == 0;
    let mut off = 0u64;
    while adelanto && off < 1 << 20 {
        let r = c.pedir(LEER, frio.asa, off, 64 * 1024, "");
        adelanto &= r.estado == 0 && r.a == 64 * 1024;
        for &b in &c.ventana()[..r.a as usize] {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            adelanto &= b == x as u8;
        }
        off += r.a;
        c.ventana_mut()[..4096].fill(off as u8);
        adelanto &= c.pedir(ESCRIBIR, otro.asa, off, 4096, "").a == 4096 && c.pedir(VACIAR, 0, 0, 0, "").estado == 0;
    }
    c.pedir(CERRAR, frio.asa, 0, 0, "");
    c.pedir(CERRAR, otro.asa, 0, 0, "");
    c.pedir(BORRAR, escrito.asa, 0, 0, "entre.bin");
    c.pedir(CERRAR, escrito.asa, 0, 0, "");
    println!("  [ficheros] A14 lectura anticipada: {}", si(adelanto));
    // A14, TRIM: 256 KB que no se comprimen (xorshift), borrados y guardados. El disco los descarta; desde fuera,
    // `disco_archivos.py` comprueba que ya no están en la imagen.
    let escrito = c.pedir(BUSCAR, 0, 0, 0, "Escrito");
    let f = c.pedir(CREAR, escrito.asa, 1, 0, "descarte.bin");
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut trim = f.estado == 0;
    for off in (0..256 * 1024u64).step_by(64 * 1024) {
        for b in c.ventana_mut()[..64 * 1024].iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        trim &= c.pedir(ESCRIBIR, f.asa, off, 64 * 1024, "").a == 64 * 1024;
    }
    trim &= c.pedir(VACIAR, 0, 0, 0, "").estado == 0;
    c.pedir(CERRAR, f.asa, 0, 0, "");
    trim &= c.pedir(BORRAR, escrito.asa, 0, 0, "descarte.bin").estado == 0 && c.pedir(VACIAR, 0, 0, 0, "").estado == 0;
    c.pedir(CERRAR, escrito.asa, 0, 0, "");
    println!("  [ficheros] A14 descarte: {}", si(trim));
    // FS8: el disco entero, datos incluidos, tras todo lo anterior: ni un problema, ni un bloque perdido.
    let k = c.pedir(COMPROBAR, 0, 1, 0, "");
    let texto = core::str::from_utf8(&c.ventana()[..k.b as usize]).unwrap_or("?");
    println!("  [ficheros] FS8 comprobación: {}{}", si(k.estado == 0 && k.a == 0 && k.c > 0), texto.trim_end());
    // A15: sujetar el disco, escribir y borrar mientras tanto (lo borrado espera sin reusarse; comprobar lo cuenta como
    // libre pendiente) y soltar. Dos veces no se sujeta, ni se suelta lo que no se sujeta.
    // Si lo tiene sujeto el comprobador (otro proceso), se espera a que suelte (hasta `ESPERA_SUJETO_US`: con el PC
    // cargado, su prueba del arranque pasó de 3 a 31 s).
    let sujetar = || {
        let desde = dashos::ahora_us();
        let mut e = c.pedir(SUJETAR, 0, 0, 0, "").estado;
        while e == EN_USO && dashos::ahora_us() - desde < ESPERA_SUJETO_US {
            dashos::dormir_us(10_000);
            e = c.pedir(SUJETAR, 0, 0, 0, "").estado;
        }
        e
    };
    let primero = sujetar();
    let segundo = c.pedir(SUJETAR, 0, 0, 0, "").estado;
    // Lo que se escribe en cada vuelta, medido con el disco ya sujeto: lo de las tres tiene que caber sin bajar del 1/16
    // con el que el servicio lo soltaría, con `RESERVA` para los demás (y para lo que se reescribe de los cajones al
    // guardar: los datos se comprimen, y 3 × 200 KB retienen unos 64 bloques, no 150). Hasta 200 KB; con menos de 64 KB
    // por vuelta no hay sitio para la prueba.
    let info = c.pedir(INFO, 0, 0, 0, "");
    let tam = (info.b.saturating_sub(info.a / 16 + RESERVA) / 3 & !4095).min(200 * 1024);
    let mut sujeto = primero == 0 && segundo == EN_USO && tam >= 64 * 1024;
    let mut escribir_bien = true;
    let escrito = c.pedir(BUSCAR, 0, 0, 0, "Escrito");
    for vuelta in 0..3 {
        if let Some(a) = hacer(&c, escrito.asa, "a15.bin", tam, vuelta) {
            // Guardado antes de borrar, para que lo borrado quede sujeto.
            escribir_bien &= c.pedir(VACIAR, 0, 0, 0, "").estado == 0;
            c.pedir(CERRAR, a, 0, 0, "");
        } else {
            escribir_bien = false;
        }
        escribir_bien &= c.pedir(BORRAR, escrito.asa, 0, 0, "a15.bin").estado == 0 && c.pedir(VACIAR, 0, 0, 0, "").estado == 0;
    }
    c.pedir(CERRAR, escrito.asa, 0, 0, "");
    let comprobar_1 = c.pedir(COMPROBAR, 0, 0, 0, "");
    let soltar_1 = c.pedir(SOLTAR, 0, 0, 0, "").estado;
    let soltar_2 = c.pedir(SOLTAR, 0, 0, 0, "").estado;
    let comprobar_2 = c.pedir(COMPROBAR, 0, 0, 0, "");
    sujeto = sujeto && escribir_bien && comprobar_1.a == 0 && soltar_1 == 0 && soltar_2 == NO_EXISTE && comprobar_2.a == 0;
    if !sujeto {
        // Para saber cuál falla (sale a veces con el PC cargado).
        println!("  [ficheros] A15 sujetar: sujetar {} y {}, {} KB libres ({} KB por vuelta), escribir {}, comprobar {} ({} problemas), soltar {} y {}, comprobar {} ({})",
            primero, segundo, info.b / 1024, tam / 1024, escribir_bien, comprobar_1.estado, comprobar_1.a, soltar_1, soltar_2, comprobar_2.estado, comprobar_2.a);
    }
    println!("  [ficheros] A15 sujetar: {}", si(sujeto));
    // A15, hostil: sujetar y no soltar mientras se escribe, guarda y borra mucho más de lo que cabe. El servicio suelta solo al
    // quedar poco sitio: ninguna escritura se queda sin sitio y el SOLTAR de después dice LLENO.
    let escrito = c.pedir(BUSCAR, 0, 0, 0, "Escrito");
    let mut lleno = sujetar() == 0;
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut rondas = 0;
    // El primer paso que falla y su estado, para el mensaje.
    let mut fallo: Option<(&str, i64)> = None;
    while lleno && rondas < 64 {
        let f = c.pedir(CREAR, escrito.asa, 1, 0, "a15b.bin");
        if f.estado != 0 && fallo.is_none() {
            fallo = Some(("crear", f.estado as i64));
        }
        lleno &= f.estado == 0;
        for off in (0..256 * 1024u64).step_by(64 * 1024) {
            for b in c.ventana_mut()[..64 * 1024].iter_mut() {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                *b = x as u8;
            }
            let w = c.pedir(ESCRIBIR, f.asa, off, 64 * 1024, "");
            if w.a != 64 * 1024 && fallo.is_none() {
                fallo = Some(("escribir", w.estado as i64));
            }
            lleno &= w.a == 64 * 1024;
        }
        // Guardado antes de borrarlo: si no, sus bloques son nuevos (ninguna foto los nombra) y se reusan sin esperar.
        let v = c.pedir(VACIAR, 0, 0, 0, "").estado;
        if v != 0 && fallo.is_none() {
            fallo = Some(("guardar", v as i64));
        }
        lleno &= v == 0;
        c.pedir(CERRAR, f.asa, 0, 0, "");
        let b = c.pedir(BORRAR, escrito.asa, 0, 0, "a15b.bin").estado;
        let v2 = c.pedir(VACIAR, 0, 0, 0, "").estado;
        if (b != 0 || v2 != 0) && fallo.is_none() {
            fallo = Some(("borrar y guardar", (b as i64) << 32 | (v2 as u32 as i64)));
        }
        lleno &= b == 0 && v2 == 0;
        rondas += 1;
    }
    c.pedir(CERRAR, escrito.asa, 0, 0, "");
    let soltar = c.pedir(SOLTAR, 0, 0, 0, "").estado;
    // El aviso de LLENO se da una vez: otro SOLTAR ya no tiene nada que soltar.
    let otra_vez = c.pedir(SOLTAR, 0, 0, 0, "").estado;
    let lleno = lleno && soltar == LLENO && otra_vez == NO_EXISTE && c.pedir(COMPROBAR, 0, 0, 0, "").a == 0;
    if !lleno {
        println!("  [ficheros] A15 sujetar sin soltar: {} rondas, soltar {} y {}, primer fallo {:?}", rondas, soltar, otra_vez, fallo);
    }
    println!("  [ficheros] A15 sujetar sin soltar: {}", si(lleno));

    // FS9: una instantánea, un fichero nuevo, volver a ella (el fichero ya no está y las asas de antes no valen) y
    // descartarla. El disco queda como estaba, sin un problema.
    let tomada = c.pedir(TOMAR, 0, 0, 0, "antes").estado == 0 && c.pedir(TOMAR, 0, 0, 0, "antes").estado == -9;
    let escrito = c.pedir(BUSCAR, 0, 0, 0, "Escrito");
    let nuevo = c.pedir(CREAR, escrito.asa, 1, 0, "temporal.txt");
    let lista = c.pedir(INSTANTANEAS, 0, 0, 0, "");
    let en_lista = lista.estado == 0 && lista.a == 1 && &c.ventana()[8..14] == b"\x05antes";
    let vuelta = c.pedir(VOLVER, 0, 0, 0, "antes").estado == 0
        && c.pedir(BUSCAR, 0, 0, 0, "Escrito/temporal.txt").estado == -2
        && c.pedir(DATOS, nuevo.asa, 0, 0, "").estado == -5
        && c.pedir(BUSCAR, 0, 0, 0, "Escrito/hola.txt").estado == 0;
    let descartada = c.pedir(DESCARTAR, 0, 0, 0, "antes").estado == 0
        && c.pedir(INSTANTANEAS, 0, 0, 0, "").a == 0
        && c.pedir(DESCARTAR, 0, 0, 0, "antes").estado == -2;
    let limpio = c.pedir(COMPROBAR, 0, 1, 0, "").a == 0;
    println!(
        "  [ficheros] FS9 instantáneas: {}",
        si(tomada && nuevo.estado == 0 && en_lista && vuelta && descartada && limpio)
    );

    // FS10: el volumen cifrado "Ana", que el disco trae cerrado y vacío. Cerrado no se lista, ni vacío: su raíz no se
    // puede leer, y sin ella no hay permisos que mirar (R6; antes, vacío se listaba).
    let lista = c.pedir(VOLUMENES, 0, 0, 0, "");
    let v = c.ventana();
    // Dos volúmenes: "Ana" y "Carla" (U3).
    let listado = lista.estado == 0 && lista.a == 2 && v[8] == 1 && v[9] == 3 && &v[10..13] == b"Ana";
    let raiz = c.pedir(VOLUMEN, 0, 0, 0, "Ana");
    let l = c.pedir(LISTAR, raiz.asa, 0, 0, "");
    let cerrado = raiz.estado == 0 && l.estado == PROTEGIDO;
    if !cerrado {
        println!("  [ficheros] Ana cerrado: raíz {}, listar {} con {}", raiz.estado, l.estado, l.a);
    }
    // Con otra contraseña no se abre; con la suya, sí.
    let abierto = c.abrir_volumen("Ana", "otra").estado == PROTEGIDO
        && c.abrir_volumen("Ana", "clave de Ana").estado == 0
        && c.pedir(VOLUMENES, 0, 0, 0, "").estado == 0
        && c.ventana()[8] == 3;
    let f = c.pedir(CREAR, raiz.asa, ARCHIVO, 0, "secreto.txt");
    c.ventana_mut()[..14].copy_from_slice(b"solo para Ana\n");
    let escrito = f.estado == 0 && c.pedir(ESCRIBIR, f.asa, 0, 14, "").a == 14 && c.pedir(CERRAR, f.asa, 0, 0, "").estado == 0;
    // Con algo abierto no se cierra; sin nada, sí, y entonces no se lee lo de dentro.
    let cerrado_otra_vez = c.pedir(CERRAR_VOLUMEN, 0, 0, 0, "Ana").estado == EN_USO
        && c.pedir(CERRAR, raiz.asa, 0, 0, "").estado == 0
        && c.pedir(CERRAR_VOLUMEN, 0, 0, 0, "Ana").estado == 0;
    let raiz = c.pedir(VOLUMEN, 0, 0, 0, "Ana");
    let sin_llave = c.pedir(BUSCAR, raiz.asa, 0, 0, "secreto.txt").estado == PROTEGIDO
        && c.pedir(LISTAR, raiz.asa, 0, 0, "").estado == PROTEGIDO;
    let releido = c.abrir_volumen("Ana", "clave de Ana").estado == 0 && {
        let h = c.pedir(BUSCAR, raiz.asa, 0, 0, "secreto.txt");
        let l = c.pedir(LEER, h.asa, 0, 64, "");
        let bien = l.a == 14 && &c.ventana()[..14] == b"solo para Ana\n";
        c.pedir(CERRAR, h.asa, 0, 0, "");
        bien
    };
    c.pedir(CERRAR, raiz.asa, 0, 0, "");
    // Uno sin cifrar: se hace, se usa y se quita (no con algo abierto).
    let hecho = c.pedir(HACER_VOLUMEN, 0, 0, 0, "Temporal");
    let t = c.pedir(VOLUMEN, 0, 0, 0, "Temporal");
    let g = c.pedir(CREAR, t.asa, ARCHIVO, 0, "x");
    // S32, su tope: 80 KB. Pasarlo da LLENO en él y no en el principal; sin tope, cabe. Lo que se escribe no se
    // comprime (si no, ocuparía un bloque).
    let tope = {
        let puesto = c.pedir(CUOTA, 0, 80 * 1024, 1, "Temporal");
        let mut x = 0x2545_f491_4f6c_dd1du64;
        for b in c.ventana_mut()[..32 * 1024].iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        let cabe = c.pedir(ESCRIBIR, g.asa, 0, 32 * 1024, "");
        let usado = c.pedir(CUOTA, 0, 0, 0, "Temporal");
        let h = c.pedir(CREAR, t.asa, ARCHIVO, 0, "y");
        let no_cabe = c.pedir(ESCRIBIR, h.asa, 0, 32 * 1024, "");
        // En el principal sí.
        let escrito = c.pedir(BUSCAR, 0, 0, 0, "Escrito");
        let p = c.pedir(CREAR, escrito.asa, ARCHIVO, 0, "tope.bin");
        let fuera = c.pedir(ESCRIBIR, p.asa, 0, 32 * 1024, "");
        c.pedir(CERRAR, p.asa, 0, 0, "");
        let borrado = c.pedir(BORRAR, escrito.asa, 0, 0, "tope.bin").estado == 0;
        c.pedir(CERRAR, escrito.asa, 0, 0, "");
        let quitado = c.pedir(CUOTA, 0, 0, 1, "Temporal");
        let ahora = c.pedir(ESCRIBIR, h.asa, 0, 32 * 1024, "");
        c.pedir(CERRAR, h.asa, 0, 0, "");
        let bien = puesto.estado == 0
            && puesto.b == 80 * 1024
            && cabe.a == 32 * 1024
            && usado.estado == 0
            && usado.a >= 32 * 1024
            && usado.a <= 80 * 1024
            && no_cabe.estado == LLENO
            && fuera.a == 32 * 1024
            && borrado
            && quitado.estado == 0
            && quitado.b == 0
            && ahora.a == 32 * 1024
            && c.pedir(CUOTA, 0, 0, 0, "").estado == MAL_FORMADO
            && c.pedir(CUOTA, 0, 0, 0, "No está").estado == NO_EXISTE;
        if !bien {
            println!("  [ficheros] S32 tope: puesto {} {}, cabe {} {}, usado {} {}, no cabe {} {}, fuera {} {}, quitado {} {}, ahora {} {}", puesto.estado, puesto.b, cabe.estado, cabe.a, usado.estado, usado.a, no_cabe.estado, no_cabe.a, fuera.estado, fuera.a, quitado.estado, quitado.b, ahora.estado, ahora.a);
        }
        bien
    };
    // S32, crecer en uso: a todo el disco (lo que ya mide, o más si la partición es mayor), y no a menos ni a más.
    let crecido = {
        let todo = c.pedir(CRECER, 0, 0, 0, "");
        let menos = c.pedir(CRECER, 0, 4096 * 1024, 0, "");
        let mas = c.pedir(CRECER, 0, 1 << 50, 0, "");
        let bien = todo.estado == 0 && todo.a >= 4096 * 1024 && menos.estado == MAL_FORMADO && mas.estado == MAL_FORMADO;
        if !bien {
            println!("  [ficheros] S32 crecer: todo {} {}, menos {}, más {}", todo.estado, todo.a, menos.estado, mas.estado);
        }
        bien
    };
    let quitado = hecho.estado == 0
        && tope
        && crecido
        && g.estado == 0
        && c.pedir(QUITAR_VOLUMEN, 0, 0, 0, "Temporal").estado == EN_USO
        && c.pedir(CERRAR, g.asa, 0, 0, "").estado == 0
        && c.pedir(CERRAR, t.asa, 0, 0, "").estado == 0
        && c.pedir(QUITAR_VOLUMEN, 0, 0, 0, "Temporal").estado == 0
        && c.pedir(VOLUMEN, 0, 0, 0, "Temporal").estado == -2;
    // R10: uno cifrado hecho aquí, con llaves del azar del kernel: abierto al hacerlo, se escribe, se cierra, no se abre
    // con otra contraseña y sí con la suya, y se quita.
    c.ventana_mut()[..10].copy_from_slice(b"clave Beto");
    let hecho = c.pedir(HACER_VOLUMEN, 0, 2, 10, "Beto");
    let cifrado_aqui = if hecho.estado == -14 {
        println!("  [ficheros] FS10 cifrado hecho aquí: sin azar de calidad");
        true
    } else {
        let b = c.pedir(VOLUMEN, 0, 0, 0, "Beto");
        let f = c.pedir(CREAR, b.asa, ARCHIVO, 0, "x");
        let bien = hecho.estado == 0
            && c.ventana()[..10].iter().all(|&x| x == 0)
            && f.estado == 0
            && c.pedir(CERRAR, f.asa, 0, 0, "").estado == 0
            && c.pedir(CERRAR, b.asa, 0, 0, "").estado == 0
            && c.pedir(CERRAR_VOLUMEN, 0, 0, 0, "Beto").estado == 0
            && c.abrir_volumen("Beto", "otra").estado == PROTEGIDO
            && c.abrir_volumen("Beto", "clave Beto").estado == 0
            && c.pedir(QUITAR_VOLUMEN, 0, 0, 0, "Beto").estado == 0;
        println!("  [ficheros] FS10 cifrado hecho aquí: {}", si(bien));
        bien
    };
    // Lo deja cerrado, y el disco sin un problema.
    let limpio = c.pedir(CERRAR_VOLUMEN, 0, 0, 0, "Ana").estado == 0 && c.pedir(COMPROBAR, 0, 1, 0, "").a == 0;
    let partes = [listado, cerrado, abierto, escrito, cerrado_otra_vez, sin_llave, releido, quitado, limpio, cifrado_aqui];
    println!("  [ficheros] FS10 volúmenes: {}", si(partes.iter().all(|&p| p)));
    if !partes.iter().all(|&p| p) {
        // Cuál falla, en el orden de arriba.
        println!("  [ficheros] FS10 por partes: {:?}", partes);
    }
    memoria(&c);
    0
}

/// "f000" a "f099": los nombres de los ficheros de la prueba de memoria (sin montón en este programa).
fn nombre_f(i: u32, b: &mut [u8; 4]) -> &str {
    *b = [b'f', b'0' + (i / 100 % 10) as u8, b'0' + (i / 10 % 10) as u8, b'0' + (i % 10) as u8];
    core::str::from_utf8(b).unwrap()
}

/// El montón del servicio, de su `PERFIL` (rama `archivos-memoria`): KB en uso y reservados, pedidos que no cupieron
/// y veces que gastó el colchón.
fn monton_del_servicio(c: &Cliente) -> Option<(u64, u64, u64, u64)> {
    let r = c.pedir(98, 0, 0, 0, "");
    let texto = core::str::from_utf8(&c.ventana()[..(r.a as usize).min(4096)]).ok()?;
    let valor = |clave: &str| texto.split(' ').find_map(|p| p.strip_prefix(clave));
    let mut m = valor("monton=")?.split('/').map(|x| x.parse::<u64>().ok());
    let (en_uso, _maximo, reservado) = (m.next()??, m.next()??, m.next()??);
    Some((en_uso, reservado, valor("fallos=")?.parse().ok()?, valor("colchones=")?.parse().ok()?))
}

/// Rama `archivos-memoria`: órdenes que agotan la memoria del servicio. El Argon2 de serie (64 MiB, el de S6b) cabe y
/// su memoria vuelve; uno fuera de los topes da `MAL_FORMADO` y uno que no cabe, `SIN_MEMORIA`, también con 100
/// ficheros abiertos y escritos sin guardar; un fichero de cientos de trozos se escribe, se lee y se borra. Después,
/// todo sigue: se lee lo de antes, lo pendiente se guarda y la comprobación (con los datos) no encuentra nada.
fn memoria(c: &Cliente) {
    let si = |b: bool| if b { "bien" } else { "MAL" };
    let antes = monton_del_servicio(c);
    // El de serie: 64 MiB y tres pasadas, al hacerlo y en cada intento de abrirlo.
    c.ventana_mut()[..10].copy_from_slice(b"clave Gema");
    let t0 = dashos::ahora_us();
    let hecho = c.pedir(HACER_VOLUMEN, 0, 1, 10, "Gema");
    let t_hacer = dashos::ahora_us() - t0;
    let de_serie = if hecho.estado == SIN_AZAR {
        println!("  [ficheros] memoria: sin azar de calidad, no se hace el de 64 MiB");
        true
    } else {
        let t0 = dashos::ahora_us();
        let otra = c.abrir_volumen("Gema", "otra");
        let t_otra = dashos::ahora_us() - t0;
        let bien = hecho.estado == 0
            && c.pedir(CERRAR_VOLUMEN, 0, 0, 0, "Gema").estado == 0
            && otra.estado == PROTEGIDO
            && c.abrir_volumen("Gema", "clave Gema").estado == 0
            && c.pedir(QUITAR_VOLUMEN, 0, 0, 0, "Gema").estado == 0;
        println!("  [ficheros] memoria: volumen con Argon2 de 64 MiB: hacerlo {} ms, probar una contraseña mala {} ms", t_hacer / 1000, t_otra / 1000);
        if !bien {
            println!("  [ficheros] memoria: de serie: hacer {}, otra {}", hecho.estado, otra.estado);
        }
        bien
    };
    let despues = monton_del_servicio(c);
    // Lo de Argon2 vuelve al kernel: el montón no se queda con los 64 MB (8 MB de margen por lo que crezca lo demás).
    let devuelto = matches!((antes, despues), (Some(a), Some(d)) if d.1 < a.1 + 8 * 1024);
    // Fuera de los topes del coste (4 GiB): MAL_FORMADO, sin intentarlo.
    c.ventana_mut()[..10].copy_from_slice(b"clave Gema");
    let fuera = c.pedir(HACER_VOLUMEN, 0, 1 | 4096 << MEMORIA_ARGON2, 10, "Gema").estado;
    // Dentro de los topes pero más de lo que tiene el servicio (320 MiB; su grupo, 128 MB): SIN_MEMORIA, y nada a medias.
    c.ventana_mut()[..10].copy_from_slice(b"clave Gema");
    let enorme = c.pedir(HACER_VOLUMEN, 0, 1 | 320 << MEMORIA_ARGON2, 10, "Gema").estado;
    let nada = c.pedir(VOLUMEN, 0, 0, 0, "Gema").estado == NO_EXISTE;
    // Muchas cosas abiertas a la vez: 100 ficheros escritos sin guardar, y otra vez el enorme (el disco de la prueba
    // tiene poco sitio: 1 KB cada uno, en su nodo).
    let dir = c.pedir(BUSCAR, 0, 0, 0, "Escrito");
    let cajon = c.pedir(CREAR, dir.asa, 2, 0, "memoria");
    let mut asas = [0u32; 100];
    let mut escritos = true;
    let mut nb = [0u8; 4];
    for (i, asa) in asas.iter_mut().enumerate() {
        let f = c.pedir(CREAR, cajon.asa, 1, 0, nombre_f(i as u32, &mut nb));
        for (j, b) in c.ventana_mut()[..1024].iter_mut().enumerate() {
            *b = dato(j as u64 + i as u64);
        }
        escritos &= f.estado == 0 && c.pedir(ESCRIBIR, f.asa, 0, 1024, "").a == 1024;
        *asa = f.asa;
    }
    c.ventana_mut()[..10].copy_from_slice(b"clave Gema");
    let enorme_con_todo = c.pedir(HACER_VOLUMEN, 0, 1 | 320 << MEMORIA_ARGON2, 10, "Gema").estado;
    // Siguen ahí, se cierran y se guardan.
    let mut releidos = true;
    for (i, &asa) in asas.iter().enumerate() {
        let l = c.pedir(LEER, asa, 0, 4096, "");
        releidos &= l.a == 1024 && c.ventana()[..1024].iter().enumerate().all(|(j, &b)| b == dato(j as u64 + i as u64));
        c.pedir(CERRAR, asa, 0, 0, "");
    }
    let guardado = c.pedir(VACIAR, 0, 0, 0, "").estado == 0;
    // Un fichero de cientos de trozos: un byte en cada registro de 128 KB (comprimido, un bloque cada uno).
    let t = c.pedir(CREAR, cajon.asa, 1, 0, "trozos");
    const TROZOS: u64 = 150;
    let mut troceado = t.estado == 0;
    for k in 0..TROZOS {
        c.ventana_mut()[0] = k as u8 | 1;
        troceado &= c.pedir(ESCRIBIR, t.asa, k * 128 * 1024 + 4096, 1, "").a == 1;
    }
    for k in 0..TROZOS {
        let l = c.pedir(LEER, t.asa, k * 128 * 1024 + 4095, 2, "");
        troceado &= l.a == 2 && c.ventana()[..2] == [0, k as u8 | 1];
    }
    c.pedir(CERRAR, t.asa, 0, 0, "");
    // Se borra todo lo de la prueba (los 100 y el troceado), y el cajón.
    let mut borrado = c.pedir(BORRAR, cajon.asa, 0, 0, "trozos").estado == 0;
    for i in 0..100u32 {
        borrado &= c.pedir(BORRAR, cajon.asa, 0, 0, nombre_f(i, &mut nb)).estado == 0;
    }
    c.pedir(CERRAR, cajon.asa, 0, 0, "");
    borrado &= c.pedir(BORRAR, dir.asa, 0, 0, "memoria").estado == 0;
    c.pedir(CERRAR, dir.asa, 0, 0, "");
    // Lo de siempre sigue: LEEME.txt y una comprobación entera, con los datos.
    let f = c.pedir(BUSCAR, 0, 0, 0, "LEEME.txt");
    let leeme = c.pedir(LEER, f.asa, 0, 64, "").a == 19 && &c.ventana()[..6] == b"DashOS";
    c.pedir(CERRAR, f.asa, 0, 0, "");
    let comprobado = c.pedir(COMPROBAR, 0, 1, 0, "");
    let limpio = comprobado.estado == 0 && comprobado.a == 0;
    let al_final = monton_del_servicio(c);
    let fallos = al_final.map_or(0, |m| m.2);
    let partes = [de_serie, devuelto, fuera == MAL_FORMADO, enorme == SIN_MEMORIA, nada, escritos, enorme_con_todo == SIN_MEMORIA, releidos, guardado, troceado, borrado, leeme, limpio, fallos >= 2];
    println!("  [ficheros] memoria: Argon2 de 64 MiB {}, vuelve al kernel {}; fuera de topes {}, sin memoria {} (y con 100 abiertos {}); el disco, limpio {}", si(de_serie), si(devuelto), si(fuera == MAL_FORMADO), si(enorme == SIN_MEMORIA), si(enorme_con_todo == SIN_MEMORIA), si(limpio));
    if let (Some(a), Some(d), Some(f)) = (antes, despues, al_final) {
        println!("  [ficheros] memoria: montón del servicio (KB en uso/reservados): {}/{} antes, {}/{} tras Argon2, {}/{} al final; {} pedidos no cupieron, colchón gastado {} veces", a.0, a.1, d.0, d.1, f.0, f.1, f.2, f.3);
    }
    println!("  [ficheros] memoria: {}", si(partes.iter().all(|&p| p)));
    if !partes.iter().all(|&p| p) {
        println!("  [ficheros] memoria por partes: {:?}; fuera {}, enorme {}, con todo {}, comprobar {} {}", partes, fuera, enorme, enorme_con_todo, comprobado.estado, comprobado.a);
    }
}
