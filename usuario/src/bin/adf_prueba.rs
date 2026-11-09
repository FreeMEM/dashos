//! Prueba de S20, fase 1: un ADF montado como volumen por el servicio `imagen` (la llave 1, el canal "imagen").
//!
//! Monta `Imagenes/disquete.adf` (hecho con `tools/adf_oraculo.py --disquete`: FFS internacional de 880 KB), pide
//! los datos del volumen, lista la raíz, lee `LEEME.txt` y su comentario, lee entero `C/grande.txt` (100 KB, más de
//! 72 bloques) buscándolo sin mirar mayúsculas, busca `cajón/ÑANDÚ.txt` (solo lo encuentra el hash internacional) y
//! mira sus bits, comprueba que escribir se rechaza y pide la comprobación entera de la imagen.
//!
//! Los ficheros del disquete están en Latin-1, como los guarda el Amiga; los nombres llegan en UTF-8.

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const VA: usize = 0x40_0000_0000;
const SESION: u32 = 0;
const BUSCAR: u32 = 1;
const LEER: u32 = 2;
const ESCRIBIR: u32 = 3;
const LISTAR: u32 = 4;
const CREAR: u32 = 7;
const INFO: u32 = 12;
const COMENTARIO: u32 = 13;
const COMPROBAR: u32 = 16;
const MONTAR: u32 = 27;
const SOLO_LECTURA: i32 = -8;
const NO_EXISTE: i32 = -2;

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
}

fn ventana() -> &'static [u8] {
    unsafe { core::slice::from_raw_parts(VA as *const u8, 64 * 1024) }
}

/// La línea `i` de `C/grande.txt` en Latin-1 ("l\xEDnea i\n"); devuelve su largo.
fn linea(i: u32, b: &mut [u8; 16]) -> usize {
    b[..6].copy_from_slice(b"l\xednea ");
    let mut digitos = [0u8; 10];
    let (mut n, mut k) = (i, 0);
    loop {
        digitos[k] = b'0' + (n % 10) as u8;
        k += 1;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for j in 0..k {
        b[6 + j] = digitos[k - 1 - j];
    }
    b[6 + k] = b'\n';
    7 + k
}

fn principal() -> u64 {
    let (lectura, vuelta) = dashos::canal_crear();
    let c = Cliente { lectura };
    let si = |b: bool| if b { "bien" } else { "MAL" };
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&SESION.to_le_bytes());
    if dashos::canal_escribir(1, &p, &[vuelta]) != 0 {
        println!("  [adf-prueba] sin servicio de imágenes");
        return 1;
    }
    let mut ll = [0u32; 1];
    let Some((r, 1)) = c.leer_respuesta(&mut ll).filter(|(r, _)| r.estado == 0) else {
        println!("  [adf-prueba] sin sesión");
        return 2;
    };
    if r.a != 64 * 1024 || dashos::memoria_proyectar(ll[0], VA, true) != 0 {
        println!("  [adf-prueba] no se proyecta la ventana");
        return 3;
    }

    // Antes de montar no hay volumen; después, el disquete.
    let antes = c.pedir(BUSCAR, 0, 0, 0, "LEEME.txt");
    let m = c.pedir(MONTAR, 0, 0, 0, "Imagenes/disquete.adf");
    let otra = c.pedir(MONTAR, 0, 0, 0, "Imagenes/disquete.adf");
    println!(
        "  [adf-prueba] montar: {} ({} bloques, FFS {}, internacional {}); sin montar: {}; otra vez: {}",
        si(m.estado == 0),
        m.a,
        si(m.b & 1 != 0),
        si(m.b & 2 != 0),
        si(antes.estado == NO_EXISTE),
        si(otra.estado == -13)
    );
    if m.estado != 0 {
        return 4;
    }

    let info = c.pedir(INFO, 0, 0, 0, "");
    let v = ventana();
    let fin = v.iter().position(|&b| b == 0).unwrap_or(0);
    println!(
        "  [adf-prueba] volumen {}: {} KB, {} KB libres, bloques de {}",
        core::str::from_utf8(&v[..fin]).unwrap_or("?"),
        info.a / 1024,
        info.b / 1024,
        info.c
    );

    // La raíz, ordenada (AmigaDOS la da en el orden de su tabla de hash).
    let r = c.pedir(LISTAR, 0, 0, 0, "");
    let mut nombres: [(&str, u8); 8] = [("", 0); 8];
    let v = ventana();
    let mut off = 0;
    for e in nombres.iter_mut().take(r.a as usize) {
        let largo = v[off + 19] as usize;
        *e = (core::str::from_utf8(&v[off + 24..off + 24 + largo]).unwrap_or("?"), v[off + 18]);
        off += (24 + largo + v[off + 22] as usize).next_multiple_of(8);
    }
    let lista = &mut nombres[..(r.a as usize).min(8)];
    lista.sort_unstable();
    let mut texto = [0u8; 128];
    let mut t = 0;
    for (n, tipo) in lista.iter() {
        for b in n.bytes().chain(if *tipo == 2 { "/ " } else { " " }.bytes()) {
            texto[t] = b;
            t += 1;
        }
    }
    println!("  [adf-prueba] raíz ({} entradas, fin {}): {}", r.a, si(r.b == u64::MAX), core::str::from_utf8(&texto[..t]).unwrap().trim_end());

    // LEEME.txt y su comentario.
    let f = c.pedir(BUSCAR, 0, 0, 0, "LEEME.txt");
    let l = c.pedir(LEER, f.asa, 0, 4096, "");
    let leeme = &ventana()[..l.a as usize];
    let bien_leeme = leeme == b"Un disquete de AmigaDOS, le\xeddo en DashOS.\n";
    let k = c.pedir(COMENTARIO, f.asa, 0, 0, "");
    let comentario = core::str::from_utf8(&ventana()[..k.a as usize]).unwrap_or("?");
    println!("  [adf-prueba] LEEME.txt ({} bytes): {}; comentario {:?}", f.a, si(bien_leeme), comentario);

    // C/grande.txt entero, buscado con otras mayúsculas, por trozos de 64 KB.
    let g = c.pedir(BUSCAR, 0, 0, 0, "c/GRANDE.TXT");
    let mut esperado = [0u8; 16];
    let (mut n_linea, mut en_linea) = (0u32, 0usize);
    let mut largo = linea(n_linea, &mut esperado);
    let (mut total, mut bien) = (0u64, g.estado == 0);
    while bien && total < g.a {
        let r = c.pedir(LEER, g.asa, total, 64 * 1024, "");
        if r.estado != 0 || r.a == 0 {
            bien = false;
            break;
        }
        for &b in &ventana()[..r.a as usize] {
            if b != esperado[en_linea] {
                bien = false;
                break;
            }
            en_linea += 1;
            if en_linea == largo {
                n_linea += 1;
                en_linea = 0;
                largo = linea(n_linea, &mut esperado);
            }
        }
        total += r.a;
    }
    println!("  [adf-prueba] C/grande.txt: {} bytes, {} líneas: {}", total, n_linea, si(bien && total == g.a && n_linea == 9000));

    // Ñ y Ú solo se juntan con ñ y ú en el hash internacional. "re": sin escribir ni borrar.
    let n = c.pedir(BUSCAR, 0, 0, 0, "cajón/ÑANDÚ.txt");
    let l = c.pedir(LEER, n.asa, 0, 100, "");
    let bien_n = n.estado == 0 && &ventana()[..l.a as usize] == b"\xf1and\xfa\n";
    let modo = n.c & 0xffff;
    let bits = n.c >> 32;
    println!("  [adf-prueba] cajón/ÑANDÚ.txt: {}; sin escribir: {}; no borrar: {}", si(bien_n), si(modo & 0o200 == 0 && modo & 0o400 != 0), si(bits & 16 != 0));

    // Solo lectura.
    let e = c.pedir(ESCRIBIR, f.asa, 0, 1, "");
    let cr = c.pedir(CREAR, 0, 1, 0, "nuevo");
    println!("  [adf-prueba] escribir: {}; crear: {}", si(e.estado == SOLO_LECTURA), si(cr.estado == SOLO_LECTURA));

    let k = c.pedir(COMPROBAR, 0, 0, 0, "");
    println!("  [adf-prueba] comprobación: {} problemas: {}", k.a, si(k.estado == 0 && k.a == 0));
    0
}
