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
//! | 9 | registros_proyectar(llave, dirección): los registros de un aparato, como memoria de dispositivo |
//! | 10 | interrupcion_asociar(llave de interrupción, llave de canal): llegará como mensaje a ese canal |
//! | 11 | interrupcion_terminar(llave): ya atendida; se vuelve a habilitar |

use crate::excepciones::Marco;
use crate::objetos::{self, Canal, Mensaje, Objeto, MAX_DATOS, MAX_LLAVES};
use crate::{kprint, kprintln, planificador};
use alloc::vec::Vec;

const LIMITE_USUARIO: u64 = 1 << 39;

/// Si es falso, las salidas con código 0 no se escriben (para la prueba de C18, que crea muchos procesos).
/// Cuántos hilos de usuario han salido con `salir` (para que las pruebas sepan cuándo han acabado).
pub static SALIDAS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static SALIDAS_VISIBLES: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

const E_ARGUMENTO: u64 = -1i64 as u64;
const E_LLAVE: u64 = -2i64 as u64;
const E_DERECHOS: u64 = -3i64 as u64;
const E_CERRADO: u64 = -4i64 as u64;
const E_TAMANO: u64 = -5i64 as u64;
const E_TIPO: u64 = -6i64 as u64;
/// No queda memoria para el proceso (la última parte es del kernel).
const E_MEMORIA: u64 = -7i64 as u64;
/// Un límite por proceso o por canal: tabla de llaves llena, cola llena o demasiadas proyecciones.
const E_LIMITE: u64 = -8i64 as u64;
/// Proyecciones por proceso, hasta los cupos de S16 (F).
const MAX_PROYECCIONES: usize = 1024;

fn error_espacio(e: crate::espacio::Error) -> u64 {
    match e {
        crate::espacio::Error::SinMemoria => E_MEMORIA,
        crate::espacio::Error::Ocupada | crate::espacio::Error::Direccion => E_ARGUMENTO,
    }
}

unsafe extern "C" {
    fn desde_usuario(destino: *mut u8, origen: u64, n: usize) -> u64;
    fn hacia_usuario(destino: u64, origen: *const u8, n: usize) -> u64;
    static copia_inicio: u8;
    static copia_fin: u8;
    fn copia_fallo();
}

/// Si `elr` es un acceso de las copias de `copia.s`, a dónde hay que volver para que la copia devuelva error (C31).
pub fn fallo_de_copia(elr: u64) -> Option<u64> {
    let (inicio, fin) = unsafe { (&copia_inicio as *const u8 as u64, &copia_fin as *const u8 as u64) };
    (inicio..fin).contains(&elr).then(|| copia_fallo as *const () as u64)
}

/// Copia `n` bytes de la memoria del proceso actual. `None` si el tramo no es del usuario o no se puede leer con
/// los permisos del proceso (C31). El kernel nunca toca la memoria de un proceso de otra forma.
fn leer_usuario(p: u64, n: usize) -> Option<Vec<u8>> {
    tramo(p, n as u64)?;
    let mut v = Vec::new();
    v.try_reserve_exact(n).ok()?;
    v.resize(n, 0);
    (con_espacio(|| unsafe { desde_usuario(v.as_mut_ptr(), p, n) }) == 0).then_some(v)
}

/// Pone la mitad baja del proceso solo mientras dura una copia (PAN por software, C34). El resto del tiempo el kernel
/// tiene TTBR0 vacío, así que un fallo que siga un puntero de usuario sin pasar por aquí no encuentra nada.
fn con_espacio(copia: impl FnOnce() -> u64) -> u64 {
    let ttbr0 = planificador::ttbr0_actual();
    if ttbr0 == 0 {
        return 1;
    }
    crate::espacio::activar(ttbr0);
    let r = copia();
    crate::espacio::activar_vacio();
    r
}

/// Copia `datos` a la memoria del proceso actual, con sus permisos. Falso si no se puede (C31).
fn escribir_usuario(p: u64, datos: &[u8]) -> bool {
    tramo(p, datos.len() as u64).is_some() && con_espacio(|| unsafe { hacia_usuario(p, datos.as_ptr(), datos.len()) }) == 0
}

/// Una tabla de números de llave en el orden de bytes del proceso (C30).
fn llaves_a_bytes(numeros: &[u32], big_endian: bool) -> Vec<u8> {
    numeros.iter().flat_map(|&n| if big_endian { n.to_be_bytes() } else { n.to_le_bytes() }).collect()
}

fn llaves_de_bytes(b: &[u8], big_endian: bool) -> Vec<u32> {
    b.chunks_exact(4)
        .map(|c| {
            let c = [c[0], c[1], c[2], c[3]];
            if big_endian { u32::from_be_bytes(c) } else { u32::from_le_bytes(c) }
        })
        .collect()
}

/// Comprueba que un tramo de memoria es del usuario.
fn tramo(p: u64, n: u64) -> Option<(u64, usize)> {
    match p.checked_add(n) {
        Some(fin) if fin <= LIMITE_USUARIO => Some((p, n as usize)),
        _ => None,
    }
}

pub fn atender(marco: &mut Marco) {
    // Las pilas de los hilos muertos de este núcleo se liberan también aquí, no solo desde el hilo ocioso, que en un
    // núcleo siempre ocupado no corre nunca.
    planificador::recoger();
    let a = marco.x;
    let r = match a[8] {
        1 => escribir_consola(a[0], a[1]),
        2 => {
            if a[0] != 0 || SALIDAS_VISIBLES.load(core::sync::atomic::Ordering::Relaxed) {
                kprintln!("  [hilo {}] sale con código {}", planificador::actual_id(), a[0]);
            }
            SALIDAS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            planificador::terminar_actual();
        }
        3 => canal_crear(marco),
        // Solo para la prueba de C34 (sale de la imagen con las pruebas, C36): 1 si el kernel ve la dirección de
        // usuario fuera de una copia. Tiene que dar 0.
        99 => crate::mapa::traducir(a[0] as usize).0 as u64,
        4 => canal_escribir(a[0] as u32, a[1], a[2], a[3], a[4]),
        5 => canal_leer(marco, a[0] as u32, a[1], a[2], a[3], a[4]),
        6 => cerrar(a[0] as u32),
        7 => memoria_crear(a[0]),
        8 => memoria_proyectar(a[0] as u32, a[1], a[2]),
        9 => registros_proyectar(a[0] as u32, a[1]),
        10 => interrupcion_asociar(a[0] as u32, a[1] as u32),
        11 => interrupcion_terminar(a[0] as u32),
        n => {
            kprintln!("  [hilo {}] llamada desconocida {}", planificador::actual_id(), n);
            E_ARGUMENTO
        }
    };
    marco.x[0] = r;
}

fn escribir_consola(p: u64, n: u64) -> u64 {
    let n = n.min(4096);
    let Some(texto) = leer_usuario(p, n as usize) else { return E_ARGUMENTO };
    // El texto nativo de DashOS es UTF-8 (S4b). Un byte que no lo sea sale como "?", sin perder el resto. Los
    // caracteres de control, salvo el salto de línea y el tabulador, también: así un proceso no manda secuencias
    // de escape al terminal (auditoría, bajos).
    let mut limpio = alloc::string::String::new();
    if limpio.try_reserve(texto.len()).is_err() {
        return E_MEMORIA;
    }
    for trozo in texto.utf8_chunks() {
        for c in trozo.valid().chars() {
            limpio.push(if c.is_control() && c != '\n' && c != '\t' { '?' } else { c });
        }
        for _ in trozo.invalid() {
            limpio.push('?');
        }
    }
    kprint!("{}", limpio);
    n as u64
}

fn canal_crear(marco: &mut Marco) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let mut t = proc_.llaves.tomar();
    if t.huecos() < 2 {
        return E_LIMITE;
    }
    let Some((l0, l1)) = Canal::nuevo_de(&proc_.canales) else { return E_LIMITE };
    // Con hueco para las dos, instalar solo falla sin memoria para la tabla; lo que llegó a entrar se quita.
    let Ok(h0) = t.instalar(l0) else { return E_MEMORIA };
    let Ok(h1) = t.instalar(l1) else {
        t.quitar(h0);
        return E_MEMORIA;
    };
    marco.x[1] = h1 as u64;
    h0 as u64
}

fn canal_escribir(h: u32, datos: u64, largo: u64, llaves: u64, n_llaves: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if largo as usize > MAX_DATOS || n_llaves as usize > MAX_LLAVES {
        return E_TAMANO;
    }
    let (Some(datos), Some(tabla)) = (leer_usuario(datos, largo as usize), leer_usuario(llaves, 4 * n_llaves as usize))
    else {
        return E_ARGUMENTO;
    };
    let Some(destino) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    if destino.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    let Objeto::Extremo(canal, lado) = destino.objeto else { return E_TIPO };
    // Las llaves que viajan salen de la tabla de quien manda. Hacen falta derechos de transferir.
    let numeros = llaves_de_bytes(&tabla, proc_.big_endian);
    let mut viajan = Vec::new();
    {
        let mut t = proc_.llaves.tomar();
        for (i, &n) in numeros.iter().enumerate() {
            // Una llave repetida en la tabla es un error, no un pánico (auditoría, hallazgo 6).
            if numeros[..i].contains(&n) {
                return E_ARGUMENTO;
            }
            match t.ver(n) {
                Some(l) if l.derechos & objetos::TRANSFERIR != 0 && n != h => {}
                Some(_) => return E_DERECHOS,
                None => return E_LLAVE,
            }
        }
        for &n in &numeros {
            let Some(l) = t.quitar(n) else { unreachable!() };
            viajan.push(l);
        }
    }
    let m = Mensaje { datos, llaves: viajan };
    match canal.escribir(lado, m) {
        Ok(()) => 0,
        Err((error, m)) => {
            // No se pudo mandar: las llaves vuelven a sus huecos.
            let mut t = proc_.llaves.tomar();
            for (&n, l) in numeros.iter().zip(m.llaves) {
                t.reponer(n, l);
            }
            match error {
                objetos::Error::Cerrado => E_CERRADO,
                objetos::Error::Llena => E_LIMITE,
                objetos::Error::SinMemoria => E_MEMORIA,
            }
        }
    }
}

fn canal_leer(marco: &mut Marco, h: u32, buf: u64, cap: u64, llaves: u64, cap_llaves: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(bytes_llaves) = cap_llaves.checked_mul(4) else { return E_ARGUMENTO };
    if tramo(buf, cap).is_none() || tramo(llaves, bytes_llaves).is_none() {
        return E_ARGUMENTO;
    }
    let Some(origen) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    if origen.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    let Objeto::Extremo(canal, lado) = origen.objeto else { return E_TIPO };
    let m = match canal.leer(lado) {
        Ok(m) => m,
        Err(_) => return E_CERRADO,
    };
    // Si no cabe o no se puede escribir donde dice el proceso, el mensaje vuelve a la cola, intacto (C31).
    if m.datos.len() as u64 > cap || m.llaves.len() as u64 > cap_llaves {
        canal.devolver(lado, m);
        return E_TAMANO;
    }
    if !escribir_usuario(buf, &m.datos) {
        canal.devolver(lado, m);
        return E_ARGUMENTO;
    }
    let largo = m.datos.len() as u64;
    let mut t = proc_.llaves.tomar();
    if t.huecos() < m.llaves.len() {
        drop(t);
        canal.devolver(lado, m);
        return E_LIMITE;
    }
    let Mensaje { datos, llaves: recibidas } = m;
    let mut numeros = Vec::new();
    let mut sobrantes = Vec::new();
    for l in recibidas {
        match t.instalar(l) {
            Ok(n) => numeros.push(n),
            Err(l) => sobrantes.push(l),
        }
    }
    if !sobrantes.is_empty() || !escribir_usuario(llaves, &llaves_a_bytes(&numeros, proc_.big_endian)) {
        let error = if sobrantes.is_empty() { E_ARGUMENTO } else { E_MEMORIA };
        let mut devueltas: Vec<_> = numeros.iter().filter_map(|&n| t.quitar(n)).collect();
        devueltas.extend(sobrantes);
        drop(t);
        canal.devolver(lado, Mensaje { datos, llaves: devueltas });
        return error;
    }

    marco.x[1] = numeros.len() as u64;
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
    if proc_.llaves.tomar().huecos() == 0 {
        return E_LIMITE;
    }
    let Some(m) = objetos::Memoria::nueva(tamano as usize) else { return E_MEMORIA };
    let llave = objetos::Llave { objeto: Objeto::Memoria(alloc::sync::Arc::new(m)), derechos: objetos::TODOS };
    match proc_.llaves.tomar().instalar(llave) {
        Ok(n) => n as u64,
        Err(_) => E_LIMITE,
    }
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
    let mut e = proc_.espacio.tomar();
    if e.proyectados.len() >= MAX_PROYECCIONES {
        return E_LIMITE;
    }
    if e.proyectados.try_reserve(1).is_err() {
        return E_MEMORIA;
    }
    // Primero se comprueba todo el tramo y se crean sus tablas; después proyectar ya no puede fallar.
    if let Err(err) = e.preparar(va, m.paginas.len() as u64) {
        return error_espacio(err);
    }
    for (i, &p) in m.paginas.iter().enumerate() {
        let _ = e.proyectar(va + i as u64 * pagina, p, if escribir { Permiso::Datos } else { Permiso::SoloLectura });
    }
    e.proyectados.push(m.clone());
    0
}

fn registros_proyectar(h: u32, va: u64) -> u64 {
    use crate::espacio::Permiso;
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Registros { fisica, tamano } = l.objeto else { return E_TIPO };
    if l.derechos & (objetos::LEER | objetos::ESCRIBIR) != objetos::LEER | objetos::ESCRIBIR {
        return E_DERECHOS;
    }
    let pagina = crate::memoria::PAGINA;
    let mut e = proc_.espacio.tomar();
    if let Err(err) = e.preparar(va, tamano.div_ceil(pagina)) {
        return error_espacio(err);
    }
    let mut i = 0;
    while i < tamano {
        let _ = e.proyectar(va + i, fisica + i, Permiso::Dispositivo);
        i += pagina;
    }
    0
}

fn interrupcion_asociar(h: u32, hc: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let (irq, canal) = {
        let t = proc_.llaves.tomar();
        (t.ver(h), t.ver(hc))
    };
    let (Some(irq), Some(canal)) = (irq, canal) else { return E_LLAVE };
    let (Objeto::Interrupcion(i), Objeto::Extremo(c, lado)) = (irq.objeto, canal.objeto) else { return E_TIPO };
    // Controlar una interrupción pide el derecho de escribir en su llave, y que lleguen al canal, el de escribir en él.
    if canal.derechos & objetos::ESCRIBIR == 0 || irq.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    i.asociar(c, lado);
    0
}

fn interrupcion_terminar(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Interrupcion(i) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    i.terminar();
    0
}
