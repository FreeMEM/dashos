//! S6b, S36 (rama `admin-desarrollo`): las pruebas del encierro y de la tanda que hace `Programas/espera` según cómo
//! se lance (ver `bin/espera.rs`). Van en el mismo programa para no gastar el disco de pruebas, que va justo (con dos
//! programas más, la prueba de `ficheros` "sujetar sin soltar" se quedaba sin sitio).
//!
//! **Aplicación encerrada** (la abre `Abrir espera` del guion del Shell, tres veces: sin modo de desarrollo, con él y
//! otra vez sin él). Cuenta cada vez en su cajón (`Datos:vueltas`, que es `Home:Datos/espera`) y dice lo que ve:
//!
//! - Si va en modo de desarrollo (su tabla de nombres trae `HomeLibs:`) y si carga `LIBS:prueba.library`, que el guion
//!   deja en `Home:Libs`: sin el modo no la encuentra (S6b, regla 6); con él, sí, de `HomeLibs:`. Bien si las dos cosas
//!   van juntas.
//! - Lo hostil: no llega a `Home:`, `Work:` ni `RAM:`, ni por su nombre ni por las raíces 2, 3 y 4 a mano; no se puede
//!   poner a sí misma en modo de desarrollo (`PERMISO_CAMBIAR`), ni escribir lo concedido; y un ayudante suyo que pide
//!   su `PROGDIR:` en `Home:` nace sin él (no sale del encierro por su ayudante).
//!
//! **Prueba hostil de la tanda** (`tanda`, lanzada por el guion del Shell con su ficha).

use alloc::vec::Vec;
use dashos::asignaciones::{self as asig, Tabla};
use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;

const SESIONES: u32 = 1;
const ARCHIVOS: u32 = 2;
const VA_VENTANA: usize = 0x40_0000_0000;
const VA_PROGRAMA: usize = 0x48_0000_0000;

/// Su sesión con el servicio de archivos (como la de `ram`).
pub struct Archivos {
    lectura: u32,
    tam: usize,
}

impl Archivos {
    pub fn abrir() -> Option<Archivos> {
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

/// Lo que hay en el fichero `camino`, entero (por trozos de la ventana), o el estado.
pub fn leer_todo(arch: &Archivos, t: &Tabla, camino: &[u8]) -> Result<Vec<u8>, i32> {
    let (r, _) = asig::abrir(&mut PorNombres(arch), t, camino)?;
    let tam = r.a as usize;
    let mut v = Vec::with_capacity(tam);
    while v.len() < tam {
        let x = arch.orden(a::LEER, r.asa, v.len() as u64, (tam - v.len()).min(arch.tam) as u64, b"");
        if x.e != a::BIEN || x.a == 0 {
            break;
        }
        v.extend_from_slice(&arch.ventana()[..x.a as usize]);
    }
    arch.cerrar(r.asa);
    if v.len() == tam { Ok(v) } else { Err(a::ERROR_DEL_DISCO) }
}

/// Pide algo a `sesiones` y lee su respuesta.
pub fn pedir(m: &[u8], mas: &[u32]) -> Option<[u8; 16]> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut llaves = [suyo, 0];
    llaves[1..1 + mas.len()].copy_from_slice(mas);
    let mut r = [0u8; 16];
    let n = if dashos::canal_escribir(SESIONES, m, &llaves[..1 + mas.len()]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 10_000_000).0 } else { -1 };
    dashos::cerrar(lectura);
    (n >= 4).then_some(r)
}

/// El ayudante (nace sin `PROGDIR:`): intenta llegar a `Home:` por las raíces que tendría si su `PROGDIR:` estuviera
/// ahí (la 5) y por las de siempre.
pub fn ayudante(arch: &Archivos) -> u64 {
    let llega = [s::RAIZ_PROGDIR_AYUDANTE, s::RAIZ_HOME, s::RAIZ_RAM, s::RAIZ_WORK].iter().filter(|&&r| arch.orden(a::BUSCAR, r as u32, 1, 0, b"").e == a::BIEN || arch.esta(r as u32, b"Prefs") == a::BIEN).count();
    println!("  [encerrada] su ayudante, que pidió su PROGDIR: en Home:, llega a {} raíces de fuera: {}", llega, if llega == 0 { "bien" } else { "MAL" });
    0
}

/// El papel de aplicación encerrada (abierta con `Abrir`).
pub fn encerrada(arch: &Archivos, t: &Tabla) -> u64 {
    let (arch, t) = (arch, t.clone());
    let mut fs = PorNombres(arch);
    // Cuántas veces se ha abierto, en su cajón.
    let vuelta = leer(arch, &t, b"Datos:vueltas").ok().and_then(|v| core::str::from_utf8(&v).ok()?.trim().parse::<u32>().ok()).unwrap_or(0) + 1;
    if let Ok((r, _)) = asig::crear(&mut fs, &t, b"Datos:vueltas", a::FICHERO) {
        arch.cerrar(r.asa);
    }
    let escrita = match asig::abrir(&mut fs, &t, b"Datos:vueltas") {
        Ok((r, _)) => {
            let e = arch.escribir(r.asa, alloc::format!("{}\n", vuelta).as_bytes());
            arch.cerrar(r.asa);
            e == a::BIEN
        }
        Err(_) => false,
    };
    // El modo de desarrollo y la biblioteca de `Home:Libs`.
    let desarrollo = t.buscar(b"HomeLibs").is_some();
    let biblioteca = leer(arch, &t, b"LIBS:prueba.library");
    let de_donde = match asig::mirar(&mut fs, &t, b"LIBS:prueba.library") {
        Ok((_, capa)) => capa,
        Err(_) => usize::MAX,
    };
    let carga = matches!(&biblioteca, Ok(b) if b.starts_with(b"biblioteca de prueba de Ana"));
    let que = match &biblioteca {
        Ok(b) => alloc::format!("carga \"{}\" (de la capa {}, HomeLibs:)", core::str::from_utf8(b).unwrap_or("?").trim(), de_donde),
        Err(e) => alloc::format!("no la encuentra ({})", e),
    };
    println!("  [encerrada] vuelta {}, modo de desarrollo: {}; LIBS:prueba.library: {}: {}", vuelta, if desarrollo { "sí" } else { "no" }, que, if carga == desarrollo && escrita && (!carga || de_donde == 0) { "bien" } else { "MAL" });
    // El encierro: ni por nombre ni a mano.
    let por_nombre = [&b"Home:Prefs/Permisos/concedidos"[..], b"Work:", b"RAM:", b"ENV:", b"Home:Libs/prueba.library"].iter().filter(|c| asig::mirar(&mut fs, &t, c).is_ok()).count();
    let a_mano = [s::RAIZ_HOME, s::RAIZ_RAM, s::RAIZ_WORK].iter().filter(|&&r| arch.orden(a::BUSCAR, r as u32, 1, 0, b"").e == a::BIEN).count();
    // Su cajón sí: escribe y lee.
    if let Ok((r, _)) = asig::crear(&mut fs, &t, b"Datos:nota", a::FICHERO) {
        arch.cerrar(r.asa);
    }
    let nota = asig::mirar(&mut fs, &t, b"Datos:nota").is_ok();
    println!("  [encerrada] encierro: llega a {} nombres de fuera y a {} raíces a mano; su cajón Datos:: {}", por_nombre, a_mano, if por_nombre == 0 && a_mano == 0 && nota { "bien" } else { "MAL" });
    // Ponerse a sí misma en modo de desarrollo: no (sin preguntar).
    let yo = b"espera";
    let mut m = [0u8; 16 + 6 + 10];
    m[0..4].copy_from_slice(&s::PERMISO_CAMBIAR.to_le_bytes());
    m[4..8].copy_from_slice(&1u32.to_le_bytes());
    m[8..12].copy_from_slice(&(yo.len() as u32).to_le_bytes());
    m[12..16].copy_from_slice(&(s::PERMISO_DESARROLLO.len() as u32).to_le_bytes());
    m[16..22].copy_from_slice(yo);
    m[22..32].copy_from_slice(s::PERMISO_DESARROLLO.as_bytes());
    let estado = pedir(&m, &[]).map_or(i32::MIN, |r| i32::from_le_bytes(r[0..4].try_into().unwrap()));
    // Y escribir lo concedido a mano: no tiene `Home:`.
    let a_mano = asig::crear(&mut fs, &t, b"Home:Prefs/Permisos/concedidos", a::FICHERO).is_ok();
    println!("  [encerrada] ponerse en modo de desarrollo: {}; escribir lo concedido: {}: {}", estado, if a_mano { "sí" } else { "no" }, if estado == s::PROTEGIDO && !a_mano { "bien" } else { "MAL" });
    // Un ayudante con su `PROGDIR:` en `Home:` (la raíz de la persona, que ella no tiene): nace sin él.
    if vuelta == 1 {
        match leer_todo(arch, &t, b"PROGDIR:espera") {
            Ok(todo) => {
                let mem = dashos::memoria_crear(todo.len().max(1));
                let lanzado = (mem as i32) > 0 && dashos::memoria_proyectar(mem, VA_PROGRAMA, true) == 0 && {
                    // SAFETY: recién proyectada con `todo.len()` bytes; se quita enseguida.
                    unsafe { core::slice::from_raw_parts_mut(VA_PROGRAMA as *mut u8, todo.len()) }.copy_from_slice(&todo);
                    dashos::memoria_quitar(VA_PROGRAMA);
                    let mut p = [0u8; 12 + 6];
                    p[0..4].copy_from_slice(&s::LANZAR_AYUDANTE.to_le_bytes());
                    p[4..8].copy_from_slice(&(todo.len() as u32).to_le_bytes());
                    p[8] = s::RAIZ_HOME;
                    p[9] = 6;
                    p[12..18].copy_from_slice(b"espera");
                    pedir(&p, &[mem]).is_some_and(|r| r[0..4] == [0; 4])
                };
                println!("  [encerrada] lanza un ayudante con su PROGDIR: en Home: ({} KB): {}", todo.len() / 1024, if lanzado { "nace" } else { "no nace" });
                // Que acabe antes que ella: si no, el guion sigue antes de que diga lo suyo.
                dashos::dormir_us(1_500_000);
            }
            Err(e) => println!("  [encerrada] no lee su PROGDIR: ({}): MAL", e),
        }
    }
    0
}

// ---------------------------------------------------------------------------------------------------------------
// La prueba hostil de la tanda de acciones de administrador (S36, decisión 1). La lanza el guion del Shell, así que
// lleva la ficha del Shell. Lo que intenta, en orden:
//
// 1. Una tanda con una acción que no es de administrador, una lista enorme (de acciones, de bytes y de memoria) y un
//    proceso que no existe: no se pregunta ni se hace nada.
// 2. Una tanda buena ("registro" y cerrar uno de sus tres procesos de prueba): mientras espera la respuesta pide otra
//    (tiene que dar `OCUPADO`) y cambia en su memoria la lista ya mandada para cerrar los otros dos: no cuenta, se hace
//    la que se enseñó. En la prueba la confirma el guion de QEMU con la contraseña de Ana.
// 3. Justo después, una acción suelta (cerrar otro) para colarla detrás: pide su propia confirmación (en la prueba,
//    `guardian` dice que no) y no se hace.
// 4. Una tanda que falla en medio (cerrar el mismo dos veces y leer el registro): se para en la segunda.
// 5. Una tanda con la contraseña mala (la teclea el guion): no se hace nada.

const VA_LISTA: usize = 0x47_0000_0000;
/// La de las tandas que se mandan y se esperan enteras (la de arriba sigue proyectada mientras tanto).
const VA_OTRA: usize = 0x4A_0000_0000;

/// Manda una tanda: la orden con `largo` y, si se da, la memoria `mem` (se va con el mensaje). El canal por donde
/// llegará la respuesta, sin esperarla.
fn mandar_tanda(largo: u32, mem: Option<u32>) -> Option<u32> {
    let (lectura, suyo) = dashos::canal_crear();
    let mut m = [0u8; 8];
    m[0..4].copy_from_slice(&s::TANDA.to_le_bytes());
    m[4..8].copy_from_slice(&largo.to_le_bytes());
    let llaves = [suyo, mem.unwrap_or(0)];
    if dashos::canal_escribir(SESIONES, &m, &llaves[..if mem.is_some() { 2 } else { 1 }]) != 0 {
        dashos::cerrar(lectura);
        return None;
    }
    Some(lectura)
}

fn esperar_tanda(lectura: u32, plazo: u64) -> Option<dashos::tanda::Resultado> {
    let mut r = [0u8; 256];
    let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], plazo);
    dashos::cerrar(lectura);
    if n > 0 { dashos::tanda::Resultado::de_bytes(&r[..n as usize]) } else { None }
}

/// Una memoria de `tam` bytes con `texto` al principio, proyectada en `va` para poder cambiarla luego; y una copia de
/// su llave, solo para leer, para mandarla.
fn memoria_con(va: usize, tam: usize, texto: &[u8]) -> Option<(u32, u32)> {
    let mem = dashos::memoria_crear(tam);
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, va, true) != 0 {
        return None;
    }
    // SAFETY: proyectada aquí con `tam` bytes, y `texto` cabe (lo dice quien llama).
    unsafe { core::slice::from_raw_parts_mut(va as *mut u8, texto.len()) }.copy_from_slice(texto);
    let copia = dashos::llave_duplicar(mem, dashos::LEER | dashos::TRANSFERIR, 0);
    (copia > 0).then_some((mem, copia as u32))
}

fn quitar_memoria(va: usize, mem: u32) {
    dashos::memoria_quitar(va);
    dashos::cerrar(mem);
}

/// Una tanda entera: mandar y esperar.
fn pedir_tanda(lista: &[u8]) -> Option<dashos::tanda::Resultado> {
    let (m, c) = memoria_con(VA_OTRA, 4096, lista)?;
    let r = mandar_tanda(lista.len() as u32, Some(c)).and_then(|l| esperar_tanda(l, 160_000_000));
    quitar_memoria(VA_OTRA, m);
    r
}

/// Si el proceso `pid` sigue (en la lista de procesos de su persona).
fn vivo(pid: u64) -> bool {
    let mut desde = 0u64;
    for _ in 0..64 {
        let mut m = [0u8; 12];
        m[0..4].copy_from_slice(&s::PROCESOS.to_le_bytes());
        m[4..12].copy_from_slice(&desde.to_le_bytes());
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0u8; 256];
        let n = if dashos::canal_escribir(SESIONES, &m, &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        if n < 8 {
            return true;
        }
        let van = (u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize).min(s::POR_PAGINA);
        let total = u32::from_le_bytes(r[4..8].try_into().unwrap()) as u64;
        if (0..van).any(|i| u64::from_le_bytes(r[8 + s::FICHA * i..16 + s::FICHA * i].try_into().unwrap()) == pid) {
            return true;
        }
        desde += van as u64;
        if van == 0 || desde >= total {
            return false;
        }
    }
    true
}

/// Lanza `programa` (ya leído) como ayudante, sin canal de arranque: su número.
fn lanzar(programa: &[u8]) -> Option<u64> {
    let mem = dashos::memoria_crear(programa.len());
    if (mem as i32) <= 0 || dashos::memoria_proyectar(mem, VA_PROGRAMA, true) != 0 {
        return None;
    }
    // SAFETY: recién proyectada aquí con `programa.len()` bytes; se quita en la línea siguiente.
    unsafe { core::slice::from_raw_parts_mut(VA_PROGRAMA as *mut u8, programa.len()) }.copy_from_slice(programa);
    dashos::memoria_quitar(VA_PROGRAMA);
    let mut p = [0u8; 12 + 6];
    p[0..4].copy_from_slice(&s::LANZAR_AYUDANTE.to_le_bytes());
    p[4..8].copy_from_slice(&(programa.len() as u32).to_le_bytes());
    p[8] = s::SIN_RAIZ;
    p[9] = 6;
    p[12..18].copy_from_slice(b"espera");
    let r = pedir(&p, &[mem])?;
    (r[0..4] == [0; 4]).then(|| u64::from_le_bytes(r[4..12].try_into().unwrap()))
}

/// El estado de una respuesta, o `i32::MIN` si no llegó.
fn e_de(r: Option<dashos::tanda::Resultado>) -> i32 {
    r.map_or(i32::MIN, |r| r.estado)
}

/// El papel de la prueba hostil de la tanda.
pub fn tanda(arch: &Archivos, t: &Tabla) -> u64 {
    use dashos::tanda::{MAX_ACCIONES, NO_HECHA};
    let mal_formado = a::MAL_FORMADO;
    let mut todo = true;
    let mut paso = |que: core::fmt::Arguments, si: bool| {
        todo &= si;
        println!("  [tanda] {}: {}", que, if si { "bien" } else { "MAL" });
    };
    // 1. Sin preguntar ni hacer nada.
    let r = pedir_tanda(b"registro\nbloquear");
    paso(format_args!("con una que no es de administrador, {}", e_de(r)), r.is_some_and(|r| r.estado == s::NO_EXISTE && r.hechas == 0));
    let diecisiete = "registro\n".repeat(MAX_ACCIONES + 1);
    let muchas = pedir_tanda(diecisiete.as_bytes()).is_some_and(|r| r.estado == mal_formado);
    let con = |tam: usize, largo: u32| {
        memoria_con(VA_OTRA, tam, b"registro").and_then(|(m, c)| {
            let r = mandar_tanda(largo, Some(c)).and_then(|l| esperar_tanda(l, 10_000_000));
            quitar_memoria(VA_OTRA, m);
            r
        })
    };
    let (larga, enorme) = (con(8192, 5000), con(1 << 20, 8));
    let sin = mandar_tanda(8, None).and_then(|l| esperar_tanda(l, 10_000_000));
    let malas = [larga, enorme, sin].iter().all(|r| r.is_some_and(|r| r.estado == mal_formado && r.hechas == 0));
    paso(format_args!("listas enormes (17 acciones, 5000 bytes, 1 MB de memoria, sin memoria): no se preguntan"), muchas && malas);
    let r = pedir_tanda(b"matar 4000000000");
    paso(format_args!("un proceso que no existe, {}", e_de(r)), r.is_some_and(|r| r.estado == s::NO_EXISTE));
    // Sus tres procesos de prueba: copias de sí mismo, que sin canal de arranque solo esperan.
    let Ok(programa) = leer_todo(arch, t, b"PROGDIR:espera") else {
        println!("  [tanda] no se lee a sí misma: MAL");
        return 1;
    };
    let v: Vec<u64> = (0..3).filter_map(|_| lanzar(&programa)).collect();
    if v.len() != 3 {
        println!("  [tanda] no se lanzan sus procesos de prueba ({} de 3): MAL", v.len());
        return 1;
    }
    let (pa, pb, pc) = (v[0], v[1], v[2]);
    // 2. Una buena; otra mientras; y la lista cambiada tras mandarla.
    let lista = alloc::format!("registro\nmatar {}\n", pa);
    let Some((mem, copia)) = memoria_con(VA_LISTA, 4096, lista.as_bytes()) else { return 1 };
    let Some(lectura) = mandar_tanda(lista.len() as u32, Some(copia)) else { return 1 };
    let otra = pedir_tanda(b"registro");
    paso(format_args!("otra mientras espera la primera, {}", e_de(otra)), otra.is_some_and(|r| r.estado == s::OCUPADO));
    // Cuando contesta la otra (`OCUPADO`), `sesiones` ya tiene la primera (atiende en orden): cambiarla ahora es
    // cambiarla tras mandarla y antes de que el usuario conteste.
    let cambiada = alloc::format!("matar {}\nmatar {}\n", pb, pc);
    // SAFETY: la memoria sigue proyectada en `VA_LISTA` (4096 bytes) y `cambiada` cabe.
    unsafe { core::slice::from_raw_parts_mut(VA_LISTA as *mut u8, cambiada.len()) }.copy_from_slice(cambiada.as_bytes());
    let r = esperar_tanda(lectura, 160_000_000);
    quitar_memoria(VA_LISTA, mem);
    let (va, vb, vc) = (vivo(pa), vivo(pb), vivo(pc));
    paso(
        format_args!("se hace la tanda enseñada, no la cambiada ({} {}; vivos: {} {} {})", e_de(r), r.map_or(0, |r| r.hechas), va, vb, vc),
        r.is_some_and(|r| r.estado == 0 && r.hechas == 2) && !va && vb && vc,
    );
    // 3. Colar otra detrás.
    let mut m = [0u8; 12 + 5 + 8];
    m[0..4].copy_from_slice(&s::ADMINISTRAR.to_le_bytes());
    m[4..8].copy_from_slice(&5u32.to_le_bytes());
    m[12..17].copy_from_slice(b"matar");
    m[17..25].copy_from_slice(&pb.to_le_bytes());
    let estado = pedir(&m, &[]).map_or(i32::MIN, |r| i32::from_le_bytes(r[0..4].try_into().unwrap()));
    paso(format_args!("colar otra detrás pide su confirmación y no se hace ({}; vivo: {})", estado, vivo(pb)), estado == s::PROTEGIDO && vivo(pb));
    // 4. Se para en la que falla.
    let r = pedir_tanda(alloc::format!("matar {}\nmatar {}\nregistro", pb, pb).as_bytes());
    let parada = r.is_some_and(|r| r.estado == s::NO_EXISTE && r.hechas == 1 && r.parada == 2 && r.de_cada[0].0 == 0 && r.de_cada[1].0 == s::NO_EXISTE && r.de_cada[2].0 == NO_HECHA);
    paso(format_args!("se para en la que falla ({} {} {}; vivo: {})", e_de(r), r.map_or(0, |r| r.hechas), r.map_or(0, |r| r.parada), vivo(pb)), parada && !vivo(pb));
    // 5. La contraseña mala.
    let r = pedir_tanda(alloc::format!("matar {}", pc).as_bytes());
    paso(format_args!("con la contraseña mala no se hace ({}; vivo: {})", e_de(r), vivo(pc)), r.is_some_and(|r| r.estado == s::PROTEGIDO && r.hechas == 0) && vivo(pc));
    // Lo suyo, fuera (sin administrador: es de su persona).
    let mut m = [0u8; 12];
    m[0..4].copy_from_slice(&s::MATAR.to_le_bytes());
    m[4..12].copy_from_slice(&pc.to_le_bytes());
    pedir(&m, &[]);
    println!("  [tanda] prueba hostil acabada: {}", if todo { "bien" } else { "MAL" });
    0
}
