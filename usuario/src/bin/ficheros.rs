//! Prueba del protocolo de ficheros (FS3, FS4): un cliente del canal "archivos" (la llave 1, con su marca de
//! `inicio`). Abre su sesión, lista la raíz, lee `LEEME.txt` y `C/grande.txt` entero por trozos de la ventana, pide
//! los datos de un fichero y del volumen, y comprueba los rechazos: lo que no existe, una asa cerrada, leer un cajón y
//! listar un fichero. Después escribe: crea un cajón y un fichero de 200 KB, lo relee, lo recorta, lo mueve, lo borra
//! abierto y deja `Escrito/hola.txt` para que `tools/disco_archivos.py --comprobar` lo vea en Linux.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x40_0000_0000;
const SESION: u32 = 0;
const BUSCAR: u32 = 1;
const LEER: u32 = 2;
const LISTAR: u32 = 4;
const DATOS: u32 = 5;
const CERRAR: u32 = 6;
const ESCRIBIR: u32 = 3;
const CREAR: u32 = 7;
const BORRAR: u32 = 8;
const RENOMBRAR: u32 = 9;
const TAMANO: u32 = 10;
const VACIAR: u32 = 11;
const INFO: u32 = 12;

#[derive(Clone, Copy)]
struct R {
    estado: i32,
    asa: u32,
    a: u64,
    b: u64,
    c: u64,
}

struct Cliente {
    lectura: u32,
    ventana: usize,
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
        let mut p = [0u8; 256];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&a.to_le_bytes());
        p[16..24].copy_from_slice(&b.to_le_bytes());
        p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        p[32..32 + nombre.len()].copy_from_slice(nombre.as_bytes());
        let malo = R { estado: i32::MIN, asa: 0, a: 0, b: 0, c: 0 };
        if dashos::canal_escribir(1, &p[..32 + nombre.len()], &[]) != 0 {
            return malo;
        }
        self.leer_respuesta(&mut []).map_or(malo, |r| r.0)
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

fn principal() -> u64 {
    let (lectura, vuelta) = dashos::canal_crear();
    let mut c = Cliente { lectura, ventana: 0 };
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

    // Listar la raíz: tamaño, fecha, modo, tipo, largo del nombre y nombre, de 8 en 8 bytes.
    let r = c.pedir(LISTAR, 0, 0, 0, "");
    let mut nombres: [(&str, u8); 8] = [("", 0); 8];
    let v = c.ventana();
    let mut off = 0;
    for e in nombres.iter_mut().take(r.a as usize) {
        let largo = v[off + 19] as usize;
        *e = (core::str::from_utf8(&v[off + 20..off + 20 + largo]).unwrap_or("?"), v[off + 18]);
        off += (20 + largo).next_multiple_of(8);
    }
    let lista = &mut nombres[..r.a as usize];
    lista.sort_unstable();
    let mut texto = [0u8; 128];
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

    // FS4: escribir. Un cajón y un fichero de 200 KB, por trozos de 64 KB.
    let libre_antes = info.b;
    let dir = c.pedir(CREAR, 0, 2, 0, "Escrito");
    let f = c.pedir(CREAR, dir.asa, 1, 0, "datos.bin");
    const TAM: u64 = 200 * 1024;
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
    0
}
