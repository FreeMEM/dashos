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
//! | 12 | proceso_crear(llave de memoria con el ELF, tamaño, llaves, n_llaves) -> llave de proceso |
//! | 13 | proceso_matar(llave de proceso) |
//! | 14 | proceso_esperar(llave de proceso) -> (clase, código): 0 salió con código, 1 lo mataron, 2 un fallo |
//! | 15 | llave_duplicar(llave, derechos, marca) -> llave nueva, con esos derechos o menos y la marca si no tenía |
//! | 16 | llave_revocable(llave) -> (llave revocable, revocador) |
//! | 17 | revocar(revocador) |
//! | 18 | grupo_crear(presupuesto en bytes) -> llave de grupo: un subgrupo del propio |
//! | 19 | grupo_matar(llave de grupo) |
//! | 20 | grupo_uso(llave de grupo) -> (límite, usado) |
//! | 21 | memoria_quitar(dirección): quita la proyección que empieza ahí |
//! | 22 | memoria_dma(permiso DMA, tamaño) -> (llave de memoria, dirección de bus) |
//! | 23 | registro_leer(llave del registro, búfer, capacidad) -> bytes: para la consola |
//! | 24 | temporizador_crear(llave de canal, primer plazo µs, periodo µs) -> llave |
//! | 25 | registros_recortar(llave de registros o del firmware, desde, tamaño) -> llave |
//! | 26 | dma_liberar(permiso DMA) -> páginas devueltas de su cuarentena |
//! | 40 | hilo_crear(entrada, pila, argumento, tls) -> número del hilo |
//! | 41 | hilo_unir(número) -> (clase, código), como proceso_esperar |
//! | 42 | esperar(dirección, valor, plazo µs) -> 0 despertado, 1 la palabra no valía eso |
//! | 43 | despertar(dirección, cuántos) -> cuántos despertó |
//! | 44 | proceso_salir(código): acaba el proceso entero |
//!
//! `proceso_crear` lleva en x4 una llave de grupo donde crear el proceso, o 0 para el propio (C42).

use crate::excepciones::Marco;
use crate::objetos::{self, Canal, Mensaje, Objeto, MAX_DATOS, MAX_LLAVES};
use crate::{kprintln, planificador};
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
/// Venció el plazo de una lectura (K7).
const E_PLAZO: u64 = -9i64 as u64;
/// Topes de trabajo por llamada (C35, S16 E): la llamada corre entera con las IRQ enmascaradas. Poner a cero 4 MB
/// lleva menos de un milisegundo en la Pi 4; 256 bytes por la serie a 115200 baudios, unos 22 ms (provisional, hasta
/// la consola como servicio).
const MAX_MEMORIA: u64 = 4 << 20;
const MAX_ESCRIBIR: u64 = 256;

fn error_espacio(e: crate::espacio::Error) -> u64 {
    match e {
        crate::espacio::Error::SinMemoria => E_MEMORIA,
        crate::espacio::Error::Limite => E_LIMITE,
        crate::espacio::Error::Ocupada | crate::espacio::Error::Direccion => E_ARGUMENTO,
    }
}

/// Copia `n` bytes de la memoria del proceso actual. `None` si el tramo no es del usuario o no se puede leer con
/// los permisos del proceso (C31). El kernel nunca toca la memoria de un proceso de otra forma.
fn leer_usuario(p: u64, n: usize) -> Option<Vec<u8>> {
    tramo(p, n as u64)?;
    let mut v = Vec::new();
    v.try_reserve_exact(n).ok()?;
    v.resize(n, 0);
    crate::base::usuario::leer(p, &mut v).then_some(v)
}

/// Copia `datos` a la memoria del proceso actual, con sus permisos. Falso si no se puede (C31).
fn escribir_usuario(p: u64, datos: &[u8]) -> bool {
    tramo(p, datos.len() as u64).is_some() && crate::base::usuario::escribir(p, datos)
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
    let a = marco.x();
    let r = match a[8] {
        1 => escribir_consola(a[0], a[1]),
        2 => {
            if a[0] != 0 || SALIDAS_VISIBLES.load(core::sync::atomic::Ordering::Relaxed) {
                kprintln!("  [hilo {}] sale con código {}", planificador::actual_id(), a[0]);
            }
            SALIDAS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            planificador::terminar_actual(objetos::Fin::Salida(a[0]));
        }
        3 => canal_crear(marco),
        // Solo con las pruebas (C34, C36): 1 si el kernel ve la dirección de usuario fuera de una copia. Tiene que
        // dar 0. En la imagen normal no existe.
        #[cfg(feature = "pruebas")]
        99 => crate::mapa::traducir(a[0] as usize).0 as u64,
        4 => canal_escribir(a[0] as u32, a[1], a[2], a[3], a[4]),
        5 => canal_leer(marco, a[0] as u32, a[1], a[2], a[3], a[4], 0),
        // K7: leer con plazo. El quinto argumento lleva en sus 16 bits bajos cuántas llaves caben y en el resto el
        // plazo en microsegundos.
        35 => canal_leer(marco, a[0] as u32, a[1], a[2], a[3], a[4] & 0xffff, a[4] >> 16),
        36 => proceso_avisar(a[0] as u32, a[1] as u32),
        37 => proceso_numero(a[0] as u32),
        38 => grupo_procesos(a[0] as u32, a[1], a[2]),
        6 => cerrar(a[0] as u32),
        7 => memoria_crear(a[0]),
        8 => memoria_proyectar(a[0] as u32, a[1], a[2]),
        9 => registros_proyectar(a[0] as u32, a[1]),
        10 => interrupcion_asociar(a[0] as u32, a[1] as u32),
        11 => interrupcion_terminar(a[0] as u32),
        12 => proceso_crear(a[0] as u32, a[1], a[2], a[3], a[4] as u32),
        13 => proceso_matar(a[0] as u32),
        14 => proceso_esperar(marco, a[0] as u32),
        15 => llave_duplicar(a[0] as u32, a[1], a[2]),
        16 => llave_revocable(marco, a[0] as u32),
        17 => revocar(a[0] as u32),
        18 => grupo_crear(a[0]),
        19 => grupo_matar(a[0] as u32),
        20 => grupo_uso(marco, a[0] as u32),
        21 => memoria_quitar(a[0]),
        22 => memoria_dma(marco, a[0] as u32, a[1]),
        23 => registro_leer(a[0] as u32, a[1], a[2]),
        24 => temporizador_crear(a[0] as u32, a[1], a[2]),
        25 => registros_recortar(a[0] as u32, a[1], a[2]),
        26 => dma_liberar(a[0] as u32),
        27 => memoria_tamano(a[0] as u32),
        // A6 (S19): el hilo que llama pasa a la clase de plazo (capacidad y periodo en microsegundos), o vuelve a la
        // justa con capacidad 0. Desde S3 pide una llave de permiso de plazo en a[0] (para volver a la justa, no).
        // E_LIMITE si su núcleo ya tiene prometido demasiado o los números no valen.
        28 => plazo(a[0] as u32, a[1], a[2]),
        // A14: la hora del calendario en nanosegundos desde 1970 (UTC); 0 si no se sabe. a[0] = 0; los demás valores,
        // reservados (ponerla pedirá una llave de permiso).
        39 => match a[0] {
            0 => crate::reloj::hora(),
            _ => E_ARGUMENTO,
        },
        // R10 (S5): hasta 256 bytes al azar en a[0], a[1] bytes; x1 = 1 si vienen de una semilla de verdad.
        34 => azar(marco, a[0], a[1]),
        // Hilos de usuario (docs/diseño/hilos-de-usuario.md).
        40 => hilo_crear(a[0], a[1], a[2], a[3]),
        41 => hilo_unir(marco, a[0]),
        42 => esperar(a[0], a[1], a[2]),
        43 => despertar(a[0], a[1]),
        44 => proceso_salir(a[0]),
        // M2b (docs/MULTITAREA_LOOP.md): dormir a[0] microsegundos (como mucho una hora), sin canal ni temporizador.
        30 => {
            planificador::dormir_us(a[0].min(3_600_000_000));
            0
        }
        // Fijar el hilo que llama a su núcleo (a[0] = 1) o soltarlo (0): no lo moverá el reparto entre núcleos (M4).
        31 => {
            planificador::fijar_actual(a[0] != 0);
            0
        }
        // M0 (docs/MULTITAREA_LOOP.md): para medir. 0 = el núcleo en que corre quien llama; 1, 2 y 3 = interrupciones,
        // cambios de hilo y microsegundos ociosos del núcleo a[1]; 4 = hilos robados entre núcleos; 5, 6 y 7 = páginas
        // de RAM, páginas libres y bytes del kernel. Nada de otros procesos.
        29 => match a[0] {
            0 => crate::nucleos::numero() as u64,
            q => planificador::contador(q, a[1] as usize).unwrap_or(E_ARGUMENTO),
        },
        n => {
            kprintln!("  [hilo {}] llamada desconocida {}", planificador::actual_id(), n);
            E_ARGUMENTO
        }
    };
    marco.poner_x0(r);
}

fn escribir_consola(p: u64, n: u64) -> u64 {
    // Como mucho MAX_ESCRIBIR bytes por llamada (C35, trabajo acotado): la serie va byte a byte con las IRQ
    // enmascaradas. Devuelve cuántos ha tomado; la biblioteca repite con el resto.
    let n = n.min(MAX_ESCRIBIR);
    let Some(texto) = leer_usuario(p, n as usize) else { return E_ARGUMENTO };
    // El texto nativo de DashOS es UTF-8 (S4b). Un byte que no lo sea sale como "?", sin perder el resto. Los
    // caracteres de control, salvo el salto de línea y el tabulador, también: así un proceso no manda secuencias
    // de escape al terminal (auditoría, bajos).
    //
    // Cada línea de un programa empieza por "│ " (C48): así no puede hacerse pasar por una línea del kernel (N6).
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let mut limpio = alloc::string::String::new();
    // Un byte de entrada da como mucho 3 de salida ("?" por un carácter de control de 2 bytes no pasa de 3; un byte
    // inválido, "?"); la marca la pone `imprimir_programa` (B3).
    if limpio.try_reserve(texto.len() * 3).is_err() {
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
    crate::serie::imprimir_programa(alloc::sync::Arc::as_ptr(&proc_) as u64, &limpio);
    crate::registro::avisar();
    n as u64
}

fn canal_crear(marco: &mut Marco) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let mut t = proc_.llaves.tomar();
    if t.huecos() < 2 {
        return E_LIMITE;
    }
    let Some((l0, l1)) = Canal::nuevo_de(&proc_.cupo) else { return E_MEMORIA };
    // Con hueco para las dos, instalar solo falla sin memoria para la tabla; lo que llegó a entrar se quita.
    let Ok(h0) = t.instalar(l0) else { return E_MEMORIA };
    let Ok(h1) = t.instalar(l1) else {
        t.quitar(h0);
        return E_MEMORIA;
    };
    marco.poner_x1(h1 as u64);
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
    let Objeto::Extremo(punta) = destino.objeto else { return E_TIPO };
    let (canal, lado) = (punta.canal().clone(), punta.lado());
    // Las llaves que viajan salen de la tabla de quien manda. Hacen falta derechos de transferir.
    let numeros = llaves_de_bytes(&tabla, proc_.big_endian);
    let mut viajan = Vec::new();
    // Sin llaves que viajen, la tabla ni se toma (K3).
    if !numeros.is_empty() {
        let mut t = proc_.llaves.tomar();
        for (i, &n) in numeros.iter().enumerate() {
            // Una llave repetida en la tabla es un error, no un pánico (auditoría, hallazgo 6).
            if numeros[..i].contains(&n) {
                return E_ARGUMENTO;
            }
            match t.ver(n) {
                // Sin ciclos (C40, hallazgo 15): ni un lado de este mismo canal, ni una interrupción que avisa a este
                // canal. Dentro de su propia cola no se soltarían nunca.
                Some(l) if objetos::forma_ciclo(&l, &canal) => return E_ARGUMENTO,
                Some(l) if l.derechos & objetos::TRANSFERIR != 0 => {}
                Some(_) => return E_DERECHOS,
                None => return E_LLAVE,
            }
        }
        for &n in &numeros {
            let Some(l) = t.quitar(n) else { unreachable!() };
            viajan.push(l);
        }
    }
    let m = Mensaje { datos, llaves: viajan, marca: destino.marca };
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
                objetos::Error::Plazo => E_PLAZO,
            }
        }
    }
}

fn canal_leer(marco: &mut Marco, h: u32, buf: u64, cap: u64, llaves: u64, cap_llaves: u64, plazo_us: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(bytes_llaves) = cap_llaves.checked_mul(4) else { return E_ARGUMENTO };
    if tramo(buf, cap).is_none() || tramo(llaves, bytes_llaves).is_none() {
        return E_ARGUMENTO;
    }
    let Some(origen) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    if origen.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    let Objeto::Extremo(punta) = origen.objeto else { return E_TIPO };
    let (canal, lado) = (punta.canal().clone(), punta.lado());
    let vence = if plazo_us == 0 {
        0
    } else {
        let cuentas = (plazo_us as u128 * crate::reloj::frecuencia() as u128 / 1_000_000).min(u64::MAX as u128 / 4) as u64;
        crate::reloj::ahora().saturating_add(cuentas.max(1))
    };
    let m = match canal.leer_hasta(lado, vence) {
        Ok(m) => m,
        // Cada cosa con su error: falta de memoria no es un canal cerrado (K7).
        Err(objetos::Error::Plazo) => return E_PLAZO,
        Err(objetos::Error::SinMemoria) => return E_MEMORIA,
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
    // Sin llaves en el mensaje, ni se toma la tabla ni se escribe nada en la del proceso (K3).
    if m.llaves.is_empty() {
        marco.poner_x1(0);
        marco.poner_x2(m.marca);
        return largo;
    }
    let mut t = proc_.llaves.tomar();
    if t.huecos() < m.llaves.len() {
        drop(t);
        canal.devolver(lado, m);
        return E_LIMITE;
    }
    let Mensaje { datos, llaves: recibidas, marca } = m;
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
        canal.devolver(lado, Mensaje { datos, llaves: devueltas, marca });
        return error;
    }

    // Las que se podían transferir una vez ya no se pueden pasar más (S4).
    for &n in &numeros {
        t.llegada(n);
    }
    marco.poner_x1(numeros.len() as u64);
    marco.poner_x2(marca);
    largo
}

fn cerrar(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    // Soltar la llave basta: si era la última de un lado de canal, el lado se cierra (C40).
    let Some(l) = proc_.llaves.tomar().quitar(h) else { return E_LLAVE };
    drop(l);
    0
}

fn memoria_crear(tamano: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if tamano == 0 || tamano > MAX_MEMORIA {
        return E_TAMANO;
    }
    if proc_.llaves.tomar().huecos() == 0 {
        return E_LIMITE;
    }
    // Sin cerrojos tomados aquí: puede cortar (S2).
    let Some(m) = objetos::Memoria::nueva_en_llamada(tamano as usize, &proc_.cupo) else { return E_MEMORIA };
    let llave = objetos::Llave::nueva(Objeto::Memoria(alloc::sync::Arc::new(m)), objetos::TODOS);
    match proc_.llaves.tomar().instalar(llave) {
        Ok(n) => n as u64,
        Err(_) => E_LIMITE,
    }
}

fn memoria_proyectar(h: u32, va: u64, permisos: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some((l, por)) = proc_.llaves.tomar().ver_con_intermediarios(h) else { return E_LLAVE };
    let Objeto::Memoria(m) = l.objeto else { return E_TIPO };
    let escribir = permisos & 2 != 0;
    if escribir && l.derechos & objetos::ESCRIBIR == 0 || l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    // Si la llave es revocable, la proyección se apunta en sus intermediarios antes de hacerla, y después se mira si
    // la revocaron entre medias: así o la ve quien revoca, o la quitamos aquí (C45).
    // Se apunta como pendiente; al acabar, hecha o retirada (A4).
    for (k, r) in por.iter().enumerate() {
        if !r.apuntar_proyeccion(&proc_, va) {
            for r in &por[..k] {
                r.retirar_proyeccion(&proc_, va);
            }
            return E_LIMITE;
        }
    }
    let via: Vec<usize> = por.iter().map(|r| r.id()).collect();
    if let Err(err) = proc_.espacio.tomar().proyectar_memoria(va, &m, escribir, &via) {
        for r in &por {
            r.retirar_proyeccion(&proc_, va);
        }
        return error_espacio(err);
    }
    for r in &por {
        r.confirmar_proyeccion(&proc_, va);
    }
    if por.iter().any(|r| r.revocado()) {
        proc_.espacio.tomar().quitar_proyeccion(va, Some(&m), None);
        return E_LLAVE;
    }
    0
}

/// Lo que mide un objeto de memoria, en bytes (W7): para no fiarse de lo que diga quien lo manda. Pide poder leerlo.
fn memoria_tamano(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some((l, _)) = proc_.llaves.tomar().ver_con_intermediarios(h) else { return E_LLAVE };
    let Objeto::Memoria(m) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    m.tamano()
}

/// Quita la proyección de un objeto de memoria que empieza en `va` (C45).
fn memoria_quitar(va: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if proc_.espacio.tomar().quitar_proyeccion(va, None, None) { 0 } else { E_ARGUMENTO }
}

fn registros_proyectar(h: u32, va: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let (fisica, tamano, firmware) = match l.objeto {
        Objeto::Registros { fisica, tamano } => (fisica, tamano, false),
        Objeto::Firmware { fisica, tamano } => (fisica, tamano, true),
        _ => return E_TIPO,
    };
    if l.derechos & (objetos::LEER | objetos::ESCRIBIR) != objetos::LEER | objetos::ESCRIBIR {
        return E_DERECHOS;
    }
    let r = if firmware {
        proc_.espacio.tomar().proyectar_firmware(va, fisica, tamano)
    } else {
        proc_.espacio.tomar().proyectar_dispositivo(va, fisica, tamano)
    };
    match r {
        Ok(()) => 0,
        Err(err) => error_espacio(err),
    }
}

fn interrupcion_asociar(h: u32, hc: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let (irq, canal) = {
        let t = proc_.llaves.tomar();
        (t.ver(h), t.ver_con_intermediarios(hc))
    };
    let (Some(irq), Some((canal, via))) = (irq, canal) else { return E_LLAVE };
    let (Objeto::Interrupcion(i), Objeto::Extremo(p)) = (irq.objeto, canal.objeto) else { return E_TIPO };
    let (c, lado) = (p.canal().clone(), p.lado());
    // Controlar una interrupción pide el derecho de escribir en su llave, y que lleguen al canal, el de escribir en él.
    if canal.derechos & objetos::ESCRIBIR == 0 || irq.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    // Con la marca y los intermediarios de la llave del canal: avisa como esa llave y deja de avisar si la revocan
    // (auditoría de la fase 4, A2).
    i.asociar(c, lado, via, canal.marca);
    0
}

/// Un temporizador que avisa al otro lado del canal de `hc` (C49). Como con las interrupciones, hace falta poder
/// escribir en ese canal.
fn temporizador_crear(hc: u32, primero_us: u64, periodo_us: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if primero_us > crate::temporizadores::MAX_US || periodo_us > crate::temporizadores::MAX_US {
        return E_ARGUMENTO;
    }
    let mut t = proc_.llaves.tomar();
    // Con sus intermediarios y su marca: el temporizador escribe como esa llave y se para si la revocan (auditoría de
    // la fase 4, A2).
    let Some((canal, via)) = t.ver_con_intermediarios(hc) else { return E_LLAVE };
    let Objeto::Extremo(p) = canal.objeto else { return E_TIPO };
    if canal.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    if t.huecos() < 1 {
        return E_LIMITE;
    }
    let Some(tm) = crate::temporizadores::Temporizador::nuevo(p.canal().clone(), p.lado(), &via, canal.marca, primero_us, periodo_us, &proc_.cupo) else {
        return E_MEMORIA;
    };
    match t.instalar(objetos::Llave::nueva(Objeto::Temporizador(tm), objetos::TODOS)) {
        Ok(h) => h as u64,
        Err(_) => E_MEMORIA,
    }
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

/// Saca de la tabla del proceso las llaves de `numeros`, todas con derecho de transferir y sin repetir. Si alguna no
/// vale, no saca ninguna.
fn sacar_llaves(proc_: &objetos::Proceso, numeros: &[u32]) -> Result<Vec<objetos::Llave>, u64> {
    let mut t = proc_.llaves.tomar();
    for (i, &n) in numeros.iter().enumerate() {
        if numeros[..i].contains(&n) {
            return Err(E_ARGUMENTO);
        }
        match t.ver(n) {
            Some(l) if l.derechos & objetos::TRANSFERIR != 0 => {}
            Some(_) => return Err(E_DERECHOS),
            None => return Err(E_LLAVE),
        }
    }
    Ok(numeros.iter().filter_map(|&n| t.quitar(n)).collect())
}

fn proceso_crear(h: u32, tamano: u64, llaves: u64, n_llaves: u64, grupo: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    // El grupo del proceso nuevo: el propio, o el de una llave de grupo con derecho de escribir (C42).
    let cupo = if grupo == 0 {
        proc_.cupo.clone()
    } else {
        let Some(g) = proc_.llaves.tomar().ver(grupo) else { return E_LLAVE };
        let Objeto::Grupo(c) = g.objeto else { return E_TIPO };
        if g.derechos & objetos::ESCRIBIR == 0 {
            return E_DERECHOS;
        }
        c
    };
    if n_llaves as usize > MAX_LLAVES {
        return E_TAMANO;
    }
    let Some(tabla) = leer_usuario(llaves, 4 * n_llaves as usize) else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Memoria(m) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    if tamano == 0 || tamano > (m.paginas() as u64) * crate::memoria::PAGINA {
        return E_TAMANO;
    }
    // La llave del proceso nuevo tiene que caber antes de lanzarlo.
    if proc_.llaves.tomar().huecos() == 0 {
        return E_LIMITE;
    }
    let mut binario = Vec::new();
    if binario.try_reserve_exact(tamano as usize).is_err() {
        return E_MEMORIA;
    }
    binario.resize(tamano as usize, 0);
    if !m.leer_en_llamada(0, &mut binario) {
        return E_TAMANO;
    }
    let numeros = llaves_de_bytes(&tabla, proc_.big_endian);
    let viajan = match sacar_llaves(&proc_, &numeros) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match crate::procesos::lanzar_en_llamada(&binario, viajan, None, cupo) {
        Ok((control, _)) => {
            let llave = objetos::Llave::nueva(Objeto::Proceso(control.clone()), objetos::TODOS);
            let instalada = proc_.llaves.tomar().instalar(llave);
            match instalada {
                Ok(n) => n as u64,
                Err(_) => {
                    // No cabía la llave (otro hilo llenó la tabla): sin llave nadie lo controlaría, así que muere. Sin
                    // el cerrojo de la tabla tomado (auditoría de la fase 3, B5).
                    planificador::matar_proceso(&control);
                    E_LIMITE
                }
            }
        }
        Err((error, devueltas)) => {
            let mut t = proc_.llaves.tomar();
            for (&n, l) in numeros.iter().zip(devueltas) {
                t.reponer(n, l);
            }
            match error {
                crate::procesos::Error::SinMemoria => E_MEMORIA,
                crate::procesos::Error::Elf(_) => E_ARGUMENTO,
            }
        }
    }
}

fn proceso_matar(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Proceso(c) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    planificador::matar_proceso(&c);
    0
}

/// K7: cuando acabe el proceso de la llave `hp` (con derecho de leer), escribir su fin por el canal `hc` (con derecho
/// de escribir), con la marca de esa llave. Si ya acabó, el aviso sale ya. Hasta `MAX_AVISOS` por proceso.
/// Llamada 37 (U6): el número del proceso de la llave `h` (con derecho de leer).
fn proceso_numero(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Proceso(c) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    c.pid
}

/// Llamada 38 (U6): los números de los procesos vivos del grupo de la llave `h`, hasta `max` u64 en `ptr`. Devuelve
/// cuántos hay (pueden ser más de los que caben). Solo con la llave del grupo: quien no la tiene no los ve.
fn grupo_procesos(h: u32, ptr: u64, max: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Grupo(g) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    let max = (max as usize).min(64);
    let mut v = Vec::new();
    let total = g.pids(&mut v, max);
    let bytes: Vec<u8> = v.iter().flat_map(|p| p.to_le_bytes()).collect();
    if !bytes.is_empty() && !escribir_usuario(ptr, &bytes) {
        return E_ARGUMENTO;
    }
    total as u64
}

fn proceso_avisar(hp: u32, hc: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let t = proc_.llaves.tomar();
    let Some(l) = t.ver(hp) else { return E_LLAVE };
    let Objeto::Proceso(c) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    let Some((canal, via)) = t.ver_con_intermediarios(hc) else { return E_LLAVE };
    drop(t);
    let Objeto::Extremo(p) = canal.objeto else { return E_TIPO };
    if canal.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    let mut vias = Vec::new();
    if vias.try_reserve(via.len()).is_err() {
        return E_MEMORIA;
    }
    vias.extend(via.iter().map(|r| r.vigia()));
    let aviso = objetos::AvisoFin { canal: p.canal().clone(), lado: p.lado(), marca: canal.marca, via: vias };
    let ya = {
        let mut e = c.estado.tomar();
        match e.fin {
            Some(f) => Some(f),
            None => {
                if e.avisos.len() >= objetos::MAX_AVISOS {
                    return E_LIMITE;
                }
                if e.avisos.try_reserve(1).is_err() {
                    return E_MEMORIA;
                }
                e.avisos.push(aviso);
                return 0;
            }
        }
    };
    if let Some(f) = ya {
        aviso.mandar(f);
    }
    0
}

fn proceso_esperar(marco: &mut Marco, h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Proceso(c) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    loop {
        let mut e = c.estado.tomar();
        // Si lo despertaron para matarlo, nadie lo quitó de la cola: se quita él.
        e.esperando.quitarme();
        if let Some(fin) = e.fin {
            let (clase, codigo) = fin.valores();
            marco.poner_x1(codigo);
            return clase;
        }
        if planificador::debe_morir() {
            return E_CERRADO;
        }
        if !e.esperando.apuntarme() {
            return E_MEMORIA;
        }
        // Las IRQ están enmascaradas en toda llamada: se puede bloquear soltando el cerrojo.
        planificador::bloquear_soltando(e);
    }
}

/// Una copia de la llave `h` con los derechos `derechos` que ya tenga (nunca más) y, si no tenía marca, la marca
/// `marca` (C41). Con marca 0 se queda la que tenga. Una llave marcada no se puede volver a marcar con otra: así quien
/// la recibe no puede hacerse pasar por otro. La copia es del mismo objeto: un lado de canal sigue abierto mientras
/// quede una.
/// La llamada 34 (R10): bytes al azar del generador del kernel.
fn azar(marco: &mut Marco, dir: u64, largo: u64) -> u64 {
    if largo > 256 {
        return E_TAMANO;
    }
    let mut b = [0u8; 256];
    crate::azar::llenar(&mut b[..largo as usize]);
    let bien = escribir_usuario(dir, &b[..largo as usize]);
    b.fill(0);
    core::hint::black_box(&b);
    if !bien {
        return E_ARGUMENTO;
    }
    marco.poner_x1(crate::azar::de_calidad() as u64);
    largo
}

/// La llamada 28 (S3): la clase de plazo, solo con el permiso.
fn plazo(h: u32, capacidad_us: u64, periodo_us: u64) -> u64 {
    if capacidad_us != 0 {
        let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
        let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
        let Objeto::PermisoPlazo = l.objeto else { return E_TIPO };
        if l.derechos & objetos::ESCRIBIR == 0 {
            return E_DERECHOS;
        }
    }
    if planificador::plazo_actual(capacidad_us, periodo_us) { 0 } else { E_LIMITE }
}

fn llave_duplicar(h: u32, derechos: u64, marca: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    // Declarada antes que la guarda de la tabla, para soltarse después de ella (B5).
    let resuelta;
    let mut t = proc_.llaves.tomar();
    // La llave sin resolver: si es revocable, la copia también lo es.
    let Some(l) = t.ver_crudo(h) else { return E_LLAVE };
    // La copia resuelta se suelta después del cerrojo de la tabla: si alguien revoca a la vez, podría ser lo último
    // de un lado de canal, y cerrarlo no debe pasar con la tabla tomada.
    resuelta = t.ver(h);
    if resuelta.is_none() {
        drop(t);
        return E_LLAVE; // revocada
    }
    if marca != 0 && l.marca != 0 && marca != l.marca {
        return E_DERECHOS;
    }
    // Los derechos, como mucho los de la original; la restricción `UNA_VEZ`, si la pide quien duplica o ya la tenía (S4).
    let una_vez = (l.derechos | derechos as u32) & objetos::UNA_VEZ;
    let nueva = objetos::Llave { objeto: l.objeto, derechos: (l.derechos & derechos as u32 & objetos::TODOS) | una_vez, marca: if marca != 0 { marca } else { l.marca } };
    match t.instalar(nueva) {
        Ok(n) => n as u64,
        Err(_) => E_LIMITE,
    }
}

/// Una copia revocable de la llave `h` y la llave que la revoca (C41). Quien da la copia se queda el revocador; al
/// usarlo, la copia y todas las que se hagan de ella dejan de valer, estén donde estén.
fn llave_revocable(marco: &mut Marco, h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    // Declarada antes que la guarda de la tabla, para soltarse después de ella (B5).
    let resuelta;
    let mut t = proc_.llaves.tomar();
    if t.huecos() < 2 || !t.reservar(2) {
        return E_LIMITE;
    }
    let Some(l) = t.ver_crudo(h) else { return E_LLAVE };
    resuelta = t.ver(h);
    if resuelta.is_none() {
        drop(t);
        return E_LLAVE;
    }
    let Some(r) = objetos::Revocable::nuevo(l.objeto, &proc_.cupo) else { return E_LIMITE };
    let r = alloc::sync::Arc::new(r);
    let copia = objetos::Llave { objeto: Objeto::Revocable(r.clone()), derechos: l.derechos, marca: l.marca };
    let revocador = objetos::Llave::nueva(Objeto::Revocador(r), objetos::TODOS);
    let (Ok(a), Ok(b)) = (t.instalar(copia), t.instalar(revocador)) else { return E_MEMORIA };
    marco.poner_x1(b as u64);
    a as u64
}

fn revocar(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Revocador(r) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    r.revocar();
    0
}

/// Un subgrupo del grupo propio con `presupuesto` bytes, que salen del propio (C42).
fn grupo_crear(presupuesto: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if proc_.llaves.tomar().huecos() == 0 {
        return E_LIMITE;
    }
    let Some(g) = proc_.cupo.hijo(presupuesto) else { return E_MEMORIA };
    match proc_.llaves.tomar().instalar(objetos::Llave::nueva(Objeto::Grupo(g), objetos::TODOS)) {
        Ok(n) => n as u64,
        Err(_) => E_LIMITE,
    }
}

fn grupo_matar(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Grupo(g) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    g.matar();
    0
}

fn grupo_uso(marco: &mut Marco, h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Grupo(g) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    let (limite, usado) = g.uso();
    marco.poner_x1(usado);
    limite
}

/// Memoria para DMA (C46), con una llave de permiso DMA con derecho de escribir: hasta 4 MB seguidos de la zona del
/// permiso, a cero y sin caché al proyectarla. Devuelve la llave del objeto y, en x1, su dirección de bus.
fn memoria_dma(marco: &mut Marco, h: u32, tamano: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::PermisoDma(permiso) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    if tamano == 0 || tamano > MAX_MEMORIA {
        return E_TAMANO;
    }
    if proc_.llaves.tomar().huecos() == 0 {
        return E_LIMITE;
    }
    let Some(m) = objetos::Memoria::nueva_dma(tamano as usize, &permiso, &proc_.cupo) else { return E_MEMORIA };
    let bus = m.bus().unwrap_or(0);
    match proc_.llaves.tomar().instalar(objetos::Llave::nueva(Objeto::Memoria(alloc::sync::Arc::new(m)), objetos::TODOS)) {
        Ok(n) => {
            marco.poner_x1(bus);
            n as u64
        }
        Err(_) => E_LIMITE,
    }
}

/// Devuelve la memoria DMA en cuarentena de un permiso (auditoría de la fase 4, M5): la llama un controlador cuando
/// ya ha reiniciado su aparato. Pide el derecho de escribir. Devuelve cuántas páginas.
fn dma_liberar(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::PermisoDma(permiso) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    permiso.liberar_cuarentena() as u64
}

/// Una llave de registros o de memoria del firmware que solo cubre un trozo, de páginas enteras (auditoría de la fase 4,
/// M4): así el buzón da al compositor solo la pantalla, no toda la memoria de la VideoCore. Con los mismos derechos.
fn registros_recortar(h: u32, desde: u64, tamano: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let mut t = proc_.llaves.tomar();
    // Una llave revocable no se recorta: la copia no pasaría por su intermediario y no se podría revocar (segunda
    // vuelta de la auditoría, B17).
    let Some((l, por)) = t.ver_con_intermediarios(h) else { return E_LLAVE };
    if !por.is_empty() {
        return E_ARGUMENTO;
    }
    let pagina = crate::memoria::PAGINA;
    let dentro = |fisica: u64, total: u64| -> Option<(u64, u64)> {
        let fin = desde.checked_add(tamano)?;
        (tamano > 0 && desde % pagina == 0 && tamano % pagina == 0 && fin <= total).then_some((fisica + desde, tamano))
    };
    let objeto = match l.objeto {
        Objeto::Registros { fisica, tamano: total } => match dentro(fisica, total) {
            Some((fisica, tamano)) => Objeto::Registros { fisica, tamano },
            None => return E_ARGUMENTO,
        },
        Objeto::Firmware { fisica, tamano: total } => match dentro(fisica, total) {
            Some((fisica, tamano)) => Objeto::Firmware { fisica, tamano },
            None => return E_ARGUMENTO,
        },
        _ => return E_TIPO,
    };
    if t.huecos() < 1 {
        return E_LIMITE;
    }
    match t.instalar(objetos::Llave::nueva(objeto, l.derechos)) {
        Ok(n) => n as u64,
        Err(_) => E_MEMORIA,
    }
}

/// Saca del registro del sistema (C48), esperando si no hay nada. Pide la llave del registro con derecho de leer. La
/// primera vez, el kernel deja el puerto serie a quien llama.
fn registro_leer(h: u32, buf: u64, cap: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Registro = l.objeto else { return E_TIPO };
    if l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    let cap = cap.min(4096) as usize;
    if cap == 0 || tramo(buf, cap as u64).is_none() {
        return E_ARGUMENTO;
    }
    let mut v = Vec::new();
    if v.try_reserve_exact(cap).is_err() {
        return E_MEMORIA;
    }
    v.resize(cap, 0);
    let Some(n) = crate::registro::sacar(&mut v) else { return E_CERRADO };
    if !escribir_usuario(buf, &v[..n]) {
        return E_ARGUMENTO;
    }
    n as u64
}

/// Llamada 40: un hilo más en el proceso (docs/diseño/hilos-de-usuario.md). Baja a EL0 en `entrada` con SP = `pila`,
/// x0 = `arg` y TPIDR_EL0 = `tls`. La entrada tiene que ser código del proceso y la pila, memoria suya que se puede
/// escribir: si no, error, no un hilo que muere al nacer. Se cobra `COSTE_HILO` al grupo hasta que alguien se una a él.
fn hilo_crear(entrada: u64, pila: u64, arg: u64, tls: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if entrada % 4 != 0 || pila % 16 != 0 || pila < 16 {
        return E_ARGUMENTO;
    }
    {
        let e = proc_.espacio.tomar();
        if !e.ejecutable(entrada) || !matches!(e.para_copiar(pila - 16, true), crate::espacio::Copia::Directa(_)) {
            return E_ARGUMENTO;
        }
    }
    {
        let e = proc_.control.estado.tomar();
        if e.matado || e.fin_pendiente.is_some() {
            return E_CERRADO;
        }
        if e.registro.len() >= objetos::MAX_HILOS {
            return E_LIMITE;
        }
    }
    let Some(cargo) = proc_.cupo.cobrar(crate::cupo::COSTE_HILO) else { return E_MEMORIA };
    let Some(pila_kernel) = crate::mapa::pila_nueva() else { return E_MEMORIA };
    drop(proc_);
    planificador::crear_hilo_usuario(entrada, pila, arg, tls, pila_kernel, cargo).unwrap_or(E_MEMORIA)
}

/// Llamada 41: espera a que acabe el hilo `id`, creado con `hilo_crear` en este mismo proceso, y da cómo acabó: clase
/// en x0 y código en x1, como `proceso_esperar`. Un número que no está en el registro del proceso (de otro proceso, del
/// kernel, ya unido, el propio) es un error: nunca se espera a un hilo ajeno.
fn hilo_unir(marco: &mut Marco, id: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if id == planificador::actual_id() {
        return E_ARGUMENTO;
    }
    let c = proc_.control.clone();
    drop(proc_);
    loop {
        let mut e = c.estado.tomar();
        // Si lo despertaron para matarlo, nadie lo quitó de la cola: se quita él.
        e.unen.quitarme();
        let Some(i) = e.registro.iter().position(|r| r.id == id) else { return E_ARGUMENTO };
        if let Some(fin) = e.registro[i].fin {
            // Fuera del registro, y con él lo cobrado, que se devuelve al soltarlo (fuera del cerrojo).
            let r = e.registro.swap_remove(i);
            drop(e);
            drop(r);
            let (clase, codigo) = fin.valores();
            marco.poner_x1(codigo);
            return clase;
        }
        if planificador::debe_morir() {
            return E_CERRADO;
        }
        if !e.unen.apuntarme() {
            return E_MEMORIA;
        }
        // Las IRQ están enmascaradas en toda llamada: se puede bloquear soltando el cerrojo.
        planificador::bloquear_soltando(e);
    }
}

/// Llamada 42: duerme mientras la palabra de 32 bits de `va` valga `valor`, como mucho `plazo_us` (0: sin plazo; como
/// mucho una hora). 0 si lo despertaron; 1 si la palabra no valía eso (no ha dormido).
fn esperar(va: u64, valor: u64, plazo_us: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if valor > u32::MAX as u64 {
        return E_ARGUMENTO;
    }
    match crate::espera::esperar(&proc_, va, valor as u32, plazo_us.min(3_600_000_000)) {
        crate::espera::Resultado::Despertado => 0,
        crate::espera::Resultado::OtroValor => 1,
        crate::espera::Resultado::Plazo => E_PLAZO,
        crate::espera::Resultado::Matado => E_CERRADO,
        crate::espera::Resultado::SinMemoria => E_MEMORIA,
        crate::espera::Resultado::Direccion => E_ARGUMENTO,
    }
}

/// Llamada 43: despierta hasta `n` hilos que esperan en la palabra de `va`. Cuántos.
fn despertar(va: u64, n: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    match crate::espera::despertar(&proc_, va, n.min(usize::MAX as u64) as usize) {
        Some(k) => k as u64,
        None => E_ARGUMENTO,
    }
}

/// Llamada 44: acaba el proceso entero con `codigo`. Los demás hilos mueren como al matarlo, pero el proceso acaba
/// como "salió con este código".
fn proceso_salir(codigo: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let control = proc_.control.clone();
    // Sin referencias de más al proceso: este hilo no vuelve, y lo que no suelte aquí no se soltaría nunca.
    drop(proc_);
    {
        let mut e = control.estado.tomar();
        if e.fin_pendiente.is_none() {
            e.fin_pendiente = Some(objetos::Fin::Salida(codigo));
        }
    }
    planificador::matar_proceso(&control);
    drop(control);
    if codigo != 0 || SALIDAS_VISIBLES.load(core::sync::atomic::Ordering::Relaxed) {
        kprintln!("  [hilo {}] acaba su proceso con código {}", planificador::actual_id(), codigo);
    }
    SALIDAS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    planificador::terminar_actual(objetos::Fin::Salida(codigo));
}
