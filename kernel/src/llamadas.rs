//! Llamadas al sistema desde EL0. Provisionales: en S11 se decidió que los programas no llamarán al kernel a mano
//! sino por una biblioteca; el número y el orden de los argumentos pueden cambiar.
//!
//! Número en x8, argumentos en x0..x5, resultado en x0 (y x1). Un error es un valor negativo.
//!
//! | n | llamada |
//! |---|---|
//! | 1 | escribir(texto, longitud): a la consola (provisional) |
//! | 2 | salir(código) |
//! | 3 | canal_crear() -> (llave0, llave1) |
//! | 4 | canal_escribir(llave, datos, longitud, llaves, n_llaves) |
//! | 5 | canal_leer(llave, búfer, capacidad, llaves, cap_llaves) -> (longitud, n_llaves) |
//! | 6 | cerrar(llave) |
//! | 7 | memoria_crear(tamaño) -> llave |
//! | 8 | memoria_proyectar(llave, dirección, permisos): 1 leer, 2 escribir |

use crate::excepciones::Marco;
use crate::objetos::{self, Canal, Mensaje, Objeto, MAX_DATOS, MAX_LLAVES};
use crate::{kprint, kprintln, planificador};
use alloc::vec::Vec;

const LIMITE_USUARIO: u64 = 1 << 39;

const E_ARGUMENTO: u64 = -1i64 as u64;
const E_LLAVE: u64 = -2i64 as u64;
const E_DERECHOS: u64 = -3i64 as u64;
const E_CERRADO: u64 = -4i64 as u64;
const E_TAMANO: u64 = -5i64 as u64;
const E_TIPO: u64 = -6i64 as u64;

/// Comprueba que un tramo de memoria es del usuario.
fn tramo(p: u64, n: u64) -> Option<(u64, usize)> {
    match p.checked_add(n) {
        Some(fin) if fin <= LIMITE_USUARIO => Some((p, n as usize)),
        _ => None,
    }
}

pub fn atender(marco: &mut Marco) {
    let a = marco.x;
    let r = match a[8] {
        1 => escribir_consola(a[0], a[1]),
        2 => {
            kprintln!("  [hilo {}] sale con código {}", planificador::actual_id(), a[0]);
            planificador::terminar_actual();
        }
        3 => canal_crear(marco),
        4 => canal_escribir(a[0] as u32, a[1], a[2], a[3], a[4]),
        5 => canal_leer(marco, a[0] as u32, a[1], a[2], a[3], a[4]),
        6 => cerrar(a[0] as u32),
        7 => memoria_crear(a[0]),
        8 => memoria_proyectar(a[0] as u32, a[1], a[2]),
        n => {
            kprintln!("  [hilo {}] llamada desconocida {}", planificador::actual_id(), n);
            E_ARGUMENTO
        }
    };
    marco.x[0] = r;
}

fn escribir_consola(p: u64, n: u64) -> u64 {
    let Some((p, n)) = tramo(p, n.min(4096)) else { return E_ARGUMENTO };
    let texto = unsafe { core::slice::from_raw_parts(p as *const u8, n) };
    kprint!("{}", core::str::from_utf8(texto).unwrap_or("?"));
    n as u64
}

fn canal_crear(marco: &mut Marco) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let (l0, l1) = Canal::nuevo();
    let mut t = proc_.llaves.tomar();
    let h0 = t.instalar(l0);
    let h1 = t.instalar(l1);
    marco.x[1] = h1 as u64;
    h0 as u64
}

fn canal_escribir(h: u32, datos: u64, largo: u64, llaves: u64, n_llaves: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if largo as usize > MAX_DATOS || n_llaves as usize > MAX_LLAVES {
        return E_TAMANO;
    }
    let (Some((datos, largo)), Some((llaves, n_llaves))) = (tramo(datos, largo), tramo(llaves, 4 * n_llaves)) else {
        return E_ARGUMENTO;
    };
    let n_llaves = n_llaves / 4;
    let Some(destino) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    if destino.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    let Objeto::Extremo(canal, lado) = destino.objeto else { return E_TIPO };
    // Las llaves que viajan salen de la tabla de quien manda. Hacen falta derechos de transferir.
    let numeros: Vec<u32> = (0..n_llaves).map(|i| unsafe { ((llaves as *const u32).add(i)).read() }).collect();
    let mut viajan = Vec::new();
    {
        let mut t = proc_.llaves.tomar();
        for &n in &numeros {
            match t.ver(n) {
                Some(l) if l.derechos & objetos::TRANSFERIR != 0 && n != h => {}
                Some(_) => return E_DERECHOS,
                None => return E_LLAVE,
            }
        }
        for &n in &numeros {
            viajan.push(t.quitar(n).unwrap());
        }
    }
    let m = Mensaje { datos: unsafe { core::slice::from_raw_parts(datos as *const u8, largo) }.to_vec(), llaves: viajan };
    match canal.escribir(lado, m) {
        Ok(()) => 0,
        Err(_) => E_CERRADO,
    }
}

fn canal_leer(marco: &mut Marco, h: u32, buf: u64, cap: u64, llaves: u64, cap_llaves: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let (Some((buf, cap)), Some((llaves, cap_llaves))) = (tramo(buf, cap), tramo(llaves, 4 * cap_llaves)) else {
        return E_ARGUMENTO;
    };
    let Some(origen) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    if origen.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    let Objeto::Extremo(canal, lado) = origen.objeto else { return E_TIPO };
    let m = match canal.leer(lado) {
        Ok(m) => m,
        Err(_) => return E_CERRADO,
    };
    if m.datos.len() > cap || m.llaves.len() > cap_llaves / 4 {
        return E_TAMANO; // el mensaje se pierde: provisional
    }
    unsafe { core::ptr::copy_nonoverlapping(m.datos.as_ptr(), buf as *mut u8, m.datos.len()) };
    let largo = m.datos.len() as u64;
    let n_llaves = m.llaves.len() as u64;
    let mut t = proc_.llaves.tomar();
    for (i, l) in m.llaves.into_iter().enumerate() {
        let n = t.instalar(l);
        unsafe { ((llaves as *mut u32).add(i)).write(n) };
    }
    marco.x[1] = n_llaves;
    largo
}

fn cerrar(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().quitar(h) else { return E_LLAVE };
    if let Objeto::Extremo(canal, lado) = l.objeto {
        canal.cerrar(lado);
    }
    0
}

fn memoria_crear(tamano: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if tamano == 0 || tamano > (64 << 20) {
        return E_TAMANO;
    }
    let Some(m) = objetos::Memoria::nueva(tamano as usize) else { return E_TAMANO };
    let llave = objetos::Llave { objeto: Objeto::Memoria(alloc::sync::Arc::new(m)), derechos: objetos::TODOS };
    proc_.llaves.tomar().instalar(llave) as u64
}

fn memoria_proyectar(h: u32, va: u64, permisos: u64) -> u64 {
    use crate::espacio::Permiso;
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Memoria(m) = l.objeto else { return E_TIPO };
    let escribir = permisos & 2 != 0;
    if escribir && l.derechos & objetos::ESCRIBIR == 0 || l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    let pagina = crate::memoria::PAGINA;
    if va % pagina != 0 || tramo(va, m.paginas.len() as u64 * pagina).is_none() {
        return E_ARGUMENTO;
    }
    let mut e = proc_.espacio.tomar();
    for (i, &p) in m.paginas.iter().enumerate() {
        e.proyectar(va + i as u64 * pagina, p, if escribir { Permiso::Datos } else { Permiso::SoloLectura });
    }
    0
}
