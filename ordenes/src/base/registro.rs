//! `Registro`: el registro común del sistema (S6, regla 2 de S36), en `Estado:Registros/`. Lo lee `sesiones`, que da a
//! cada uno lo suyo y lo del sistema; todo, solo a un administrador que lo confirma con su contraseña (`TODO`).
//!
//! `Registro [PROGRAMA nombre] [CLASE clase] [DESDE fecha] [HASTA fecha] [ULTIMAS n] [DE fichero] [TODO] [SEGUIR]`:
//! las líneas de los tres ficheros (`seguridad`, `sistema` y `programas`) juntas por la hora, en la hora local; con
//! `SEGUIR`, se queda enseñando las que llegan (Ctrl-C para acabar). Las fechas, por el principio: `2026-11-16`,
//! `2026-11-16 09`. `Registro APUNTA texto [MOTIVO texto]` deja una línea, como el programa que la lanza: la hora, el
//! programa y el usuario los pone `sesiones`, no quien escribe.

use crate::{di, Orden};
use alloc::string::String;
use alloc::vec::Vec;
use dashos::protocolo::sesiones as s;
use dashos::registro as rg;

/// Dónde se ve la memoria con las líneas que da `sesiones`.
const VA_LINEAS: usize = 0x60_0000_0000;
/// Lo que se espera a `sesiones`; con `TODO`, lo que tarda el usuario en escribir su contraseña.
const PLAZO_US: u64 = 10_000_000;
const PLAZO_ADMIN_US: u64 = 70_000_000;
/// Las que se enseñan si no se dice `ULTIMAS`.
const ULTIMAS: usize = 40;
/// Cada cuánto se mira si hay líneas nuevas con `SEGUIR`.
const CADA_US: u64 = 1_000_000;

/// Una petición a `sesiones` por la llave 1, con una llave para contestar: lo que contesta y la llave que traiga.
fn pedir(m: &[u8], plazo: u64) -> Option<(Vec<u8>, Option<u32>)> {
    let (lectura, suyo) = dashos::canal_crear();
    if dashos::canal_escribir(crate::SESIONES, m, &[suyo]) != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return None;
    }
    let mut r = [0u8; 256];
    let mut ll = [0u32; 1];
    let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, plazo);
    dashos::cerrar(lectura);
    (n >= 4).then(|| (r[..n as usize].to_vec(), (k == 1).then_some(ll[0])))
}

/// Lo que se lleva leído de un fichero: hasta qué byte y cuántas veces había rotado.
#[derive(Clone, Copy, Default)]
struct Visto {
    hasta: u64,
    generacion: u32,
}

/// Las líneas del fichero `f` (parte 0, el de ahora; 1, el `.1`) desde `desde`: (estado, generación, hasta, líneas).
fn leer(f: u32, parte: u32, desde: u64) -> Result<(u32, u64, Vec<u8>), i32> {
    let mut m = [0u8; 20];
    m[0..4].copy_from_slice(&s::REGISTRO_LEER.to_le_bytes());
    m[4..8].copy_from_slice(&f.to_le_bytes());
    m[8..12].copy_from_slice(&parte.to_le_bytes());
    m[12..20].copy_from_slice(&desde.to_le_bytes());
    let (r, llave) = pedir(&m, PLAZO_US).ok_or(dashos::protocolo::archivos::ERROR_DEL_DISCO)?;
    if r.len() < s::LARGO_REGISTRO_LEER {
        if let Some(l) = llave {
            dashos::cerrar(l);
        }
        return Err(i32::from_le_bytes(r[0..4].try_into().unwrap()));
    }
    let u64_en = |i: usize| u64::from_le_bytes(r[i..i + 8].try_into().unwrap());
    let estado = i32::from_le_bytes(r[0..4].try_into().unwrap());
    let generacion = u32::from_le_bytes(r[4..8].try_into().unwrap());
    let (hasta, largo) = (u64_en(8), u64_en(16) as usize);
    let mut lineas = Vec::new();
    if let Some(l) = llave {
        // Solo lo que mide de verdad el objeto: un `largo` que mienta no lleva fuera.
        let mide = dashos::memoria_tamano(l).max(0) as usize;
        if largo <= mide && dashos::memoria_proyectar(l, VA_LINEAS, false) == 0 {
            // SAFETY: el objeto de memoria recién proyectado, solo para leer, de `mide` bytes; se copia antes de soltarlo.
            lineas.extend_from_slice(unsafe { core::slice::from_raw_parts(VA_LINEAS as *const u8, largo) });
            dashos::memoria_quitar(VA_LINEAS);
        }
        dashos::cerrar(l);
    }
    if estado != 0 {
        return Err(estado);
    }
    Ok((generacion, hasta, lineas))
}

/// Una línea para enseñar: la hora local que se compara con las fechas, cuándo fue (para ordenar) y la línea.
struct Vista {
    cuando: u64,
    hora: String,
    linea: String,
}

/// Las líneas que pasan el filtro, de `crudo` (líneas enteras, de un fichero).
fn vistas(o: &mut Orden, crudo: &[u8], filtro: &rg::Filtro, sal: &mut Vec<Vista>) {
    for l in crudo.split(|&c| c == b'\n').filter(|l| !l.is_empty()) {
        let Ok(t) = core::str::from_utf8(l) else { continue };
        let Some(x) = rg::leer(t) else { continue };
        let (cuando, hora) = match rg::segundos(x.hora) {
            Some(utc) => (utc, shell::fecha::texto(o.s.local_s(utc))),
            None => (0, String::from(x.hora)),
        };
        if filtro.pasa(&x, &hora) {
            sal.push(Vista { cuando, hora, linea: String::from(t) });
        }
    }
}

fn ensenar(o: &mut Orden, v: &Vista) {
    if let Some(x) = rg::leer(&v.linea) {
        di!(o, "{} {} ({}) {}: {}\n", v.hora, x.programa, x.usuario, x.clase, x.texto);
    }
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::REGISTRO);
    if args.si("APUNTA") {
        return apuntar(o, &args);
    }
    let clase = args.texto("CLASE").map(String::from);
    if let Some(c) = &clase {
        if !rg::es_clase(c) {
            let todas: Vec<&str> = rg::CLASES.iter().map(|c| c.0).collect();
            o.error(format_args!("no hay líneas de la clase \"{}\"; las clases: {}", c, todas.join(", ")));
            return o.fin(shell::RC_ERROR);
        }
    }
    let solo = match args.texto("DE") {
        None => None,
        Some(f) => match rg::FICHEROS.iter().position(|x| x.eq_ignore_ascii_case(f)) {
            Some(i) => Some(i),
            None => {
                o.error(format_args!("no hay un registro \"{}\"; los hay de {}", f, rg::FICHEROS.join(", ")));
                return o.fin(shell::RC_ERROR);
            }
        },
    };
    if args.si("TODO") {
        // Una acción de administrador (S6b): `guardian` pide la contraseña; si es buena, este programa lee todo.
        let mut m = [0u8; 32];
        m[0..4].copy_from_slice(&s::ADMINISTRAR.to_le_bytes());
        m[4..8].copy_from_slice(&8u32.to_le_bytes());
        m[12..20].copy_from_slice(b"registro");
        match pedir(&m[..28], PLAZO_ADMIN_US) {
            Some((r, _)) if r[0..4] == [0; 4] => {}
            _ => {
                o.error(format_args!("leer el registro de todos pide la contraseña de un administrador, y no se dio"));
                return o.fin(shell::RC_ERROR);
            }
        }
    }
    let (programa, desde, hasta) = (args.texto("PROGRAMA").map(String::from), args.texto("DESDE").map(String::from), args.texto("HASTA").map(String::from));
    let filtro = rg::Filtro { programa: programa.as_deref(), clase: clase.as_deref(), desde: desde.as_deref(), hasta: hasta.as_deref() };
    let ultimas = match args.numero("ULTIMAS") {
        Some(n) if n >= 0 => n as usize,
        Some(_) => {
            o.error(format_args!("ULTIMAS: un número de 0 en adelante"));
            return o.fin(shell::RC_ERROR);
        }
        None if args.si("SEGUIR") => 10,
        None => ULTIMAS,
    };
    let ficheros: Vec<usize> = (0..rg::FICHEROS.len()).filter(|&i| solo.is_none_or(|x| x == i)).collect();
    let mut visto = [Visto::default(); 3];
    let mut todas = Vec::new();
    for &f in &ficheros {
        for parte in [1, 0] {
            match leer(f as u32, parte, 0) {
                Ok((generacion, h, lineas)) => {
                    if parte == 0 {
                        visto[f] = Visto { hasta: h, generacion };
                    }
                    vistas(o, &lineas, &filtro, &mut todas);
                }
                Err(e) => {
                    o.error(format_args!("{}: no se lee ({})", rg::FICHEROS[f], e));
                    return o.fin(shell::RC_ERROR);
                }
            }
        }
    }
    // Juntas por la hora (cada fichero ya va en orden: así las de la misma hora siguen en el suyo).
    todas.sort_by_key(|v| v.cuando);
    let hay = todas.len();
    for v in todas.iter().skip(hay.saturating_sub(ultimas)) {
        ensenar(o, v);
    }
    if !args.si("SEGUIR") {
        if hay == 0 {
            di!(o, "Nada en el registro{}.\n", if programa.is_some() || clase.is_some() || desde.is_some() || hasta.is_some() { " con eso" } else { "" });
        }
        return o.fin(shell::RC_BIEN);
    }
    // En vivo: lo nuevo de cada fichero; si rotó, lo que quedaba del de antes (ahora el `.1`) y el nuevo desde el
    // principio.
    loop {
        o.vaciar();
        let hasta_us = dashos::ahora_us() + CADA_US;
        while dashos::ahora_us() < hasta_us {
            o.pulso();
            dashos::dormir_us(100_000);
        }
        let mut nuevas = Vec::new();
        for &f in &ficheros {
            let Ok((generacion, h, lineas)) = leer(f as u32, 0, visto[f].hasta) else { continue };
            if generacion != visto[f].generacion {
                if let Ok((_, _, antes)) = leer(f as u32, 1, visto[f].hasta) {
                    vistas(o, &antes, &filtro, &mut nuevas);
                }
                // El de ahora, desde el principio.
                if let Ok((g, h2, desde0)) = leer(f as u32, 0, 0) {
                    vistas(o, &desde0, &filtro, &mut nuevas);
                    visto[f] = Visto { hasta: h2, generacion: g };
                }
                continue;
            }
            vistas(o, &lineas, &filtro, &mut nuevas);
            visto[f] = Visto { hasta: h, generacion };
        }
        nuevas.sort_by_key(|v| v.cuando);
        for v in &nuevas {
            ensenar(o, v);
        }
    }
}

/// `Registro APUNTA texto [MOTIVO texto] [CLASE clase]`: una línea en el registro de `programas`. La clase solo cuenta
/// para los programas del sistema (los demás apuntan "programa").
fn apuntar(o: &mut Orden, args: &shell::plantilla::Args) -> u64 {
    let que: Vec<&str> = args.varios("TEXTO").iter().map(|x| x.as_str()).collect();
    let que = que.join(" ");
    let motivo = args.texto("MOTIVO").unwrap_or("");
    if que.is_empty() {
        o.error(format_args!("APUNTA: falta qué apuntar"));
        return o.fin(shell::RC_ERROR);
    }
    let (q, mo) = (que.as_bytes(), motivo.as_bytes());
    // Lo que no cabe en un mensaje se corta aquí; `sesiones` lo limpia y lo recorta a lo suyo.
    let lq = q.len().min(200);
    let lm = mo.len().min(220 - lq);
    let mut m = [0u8; 256];
    m[0..4].copy_from_slice(&s::APUNTAR.to_le_bytes());
    m[4..8].copy_from_slice(&(lq as u32).to_le_bytes());
    m[8..12].copy_from_slice(&(lm as u32).to_le_bytes());
    let clase = args.texto("CLASE").unwrap_or("programa").as_bytes();
    m[12..12 + clase.len().min(16)].copy_from_slice(&clase[..clase.len().min(16)]);
    m[28..28 + lq].copy_from_slice(&q[..lq]);
    m[28 + lq..28 + lq + lm].copy_from_slice(&mo[..lm]);
    // `VECES`: la misma línea varias veces seguidas (para probar el ritmo: lo que va más deprisa de lo que se deja no
    // se apunta, y se dice).
    let veces = args.numero("VECES").unwrap_or(1).clamp(1, 1000) as usize;
    let (mut bien, mut deprisa) = (0, 0);
    for _ in 0..veces {
        match pedir(&m[..28 + lq + lm], PLAZO_US).map(|r| i32::from_le_bytes(r.0[0..4].try_into().unwrap())) {
            Some(0) => bien += 1,
            Some(s::OCUPADO) => deprisa += 1,
            r => {
                o.error(format_args!("no se apuntó ({:?})", r));
                return o.fin(shell::RC_ERROR);
            }
        }
    }
    if deprisa > 0 {
        o.error(format_args!("va demasiado deprisa: {} de {} no se apuntaron (se dirá cuántas se perdieron)", deprisa, veces));
        return o.fin(shell::RC_AVISO);
    }
    let _ = bien;
    o.fin(shell::RC_BIEN)
}
