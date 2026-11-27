//! Llamadas al sistema desde EL0. Provisionales: en S11 se decidió que los programas no llamarán al kernel a mano
//! sino por una biblioteca; el número y el orden de los argumentos pueden cambiar.
//!
//! Número en x8, argumentos en x0..x4, resultado en x0 (y x1, x2). Un error es un valor negativo. En x86-64 (X4),
//! número en rax, argumentos en rdi, rsi, rdx, r10 y r8, y resultado en rax (y rdx, r8): `Marco::llamada`.
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
//! | 20 | grupo_uso(llave de grupo) -> (límite, usado; x2 lo usado sin subgrupos, x3 las veces que topó) |
//! | 21 | memoria_quitar(dirección): quita la proyección que empieza ahí |
//! | 22 | memoria_dma(permiso DMA, tamaño) -> (llave de memoria, dirección de bus) |
//! | 23 | registro_leer(llave del registro, búfer, capacidad) -> bytes: para la consola |
//! | 24 | temporizador_crear(llave de canal, primer plazo µs, periodo µs) -> llave |
//! | 25 | registros_recortar(llave de registros o del firmware, desde, tamaño) -> llave |
//! | 26 | dma_liberar(permiso DMA) -> páginas devueltas de su cuarentena |
//! | 27 | memoria_tamano(llave de memoria) -> bytes |
//! | 28 | plazo(permiso de plazo, capacidad µs, periodo µs): el hilo pasa a la clase de plazo (0, vuelve a la justa) |
//! | 40 | hilo_crear(entrada, pila, argumento, tls) -> número del hilo |
//! | 41 | hilo_unir(número) -> (clase, código), como proceso_esperar |
//! | 42 | esperar(dirección, valor, plazo µs) -> 0 despertado, 1 la palabra no valía eso |
//! | 43 | despertar(dirección, cuántos) -> cuántos despertó |
//! | 44 | proceso_salir(código): acaba el proceso entero |
//! | 45 | maquina_crear(permiso, memoria, dirección física del invitado, entrada, x0) -> llave de máquina virtual (H9) |
//! | 46 | maquina_correr(máquina, informe, valor, interrupciones) -> motivo: entra en el invitado hasta que sale |
//! | 47 | maquina_memoria(máquina, memoria, dirección física del invitado): más RAM para el invitado |
//! | 48 | maquina_estado(máquina, operación, argumento, a, b): registros, aborto, GICV, otra vCPU, reiniciarla, patearla, parar ; 40 y 41 prestan un aparato a un dominio (S35) |
//!
//! En x86 (X10) las llamadas de máquinas son las mismas, con AMD-V o VT-x (`maquinas_x86.rs`).
//! | 51 | apagar(permiso, modo): 0 apaga la máquina, 1 la reinicia; no vuelve si se pudo |
//! | 39 | hora(0): la hora del calendario; hora(1, permiso, ns): ponerla, con el permiso del reloj (S13) |
//! | 64 | firmware_variable(permiso de variables, operación, búfer, capacidad) -> estado de UEFI; x1 = bytes: solo x86 (S32) |
//! | 59 | puertos_permitir(llave de puertos): en x86, el proceso puede usar esos puertos de E/S (X6; fue la 29 y la 56, que en `revision` son `medir` y `llaves_nombres`) |
//!
//! | 56 | llaves_nombres(búfer, capacidad) -> cuántas llaves de nacimiento tienen nombre; los copia al búfer |
//! | 61 | fallo_manejador(entrada, pila, tamaño): los fallos de EL0 del hilo van a su manejador (K5; solo ARM) |
//! | 62 | reanudar(marco): vuelve de un fallo con los registros del marco (K5; solo ARM) |
//!
//! `proceso_crear` lleva en x4 una llave de grupo donde crear el proceso, o 0 para el propio (C42). Con el bit 16 de
//! x3 (`CON_NOMBRES`), tras los números de llave van sus nombres, 16 bytes por llave: lo que es cada una.

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
/// Sin hipervisor (H9): el kernel no arrancó en EL2, así que no hay máquinas virtuales.
const E_NO_HAY: u64 = -10i64 as u64;
/// Topes de trabajo por llamada (C35, S16 E): la llamada corre entera con las IRQ enmascaradas. Poner a cero 4 MB
/// lleva menos de un milisegundo en la Pi 4, y desde X6 se para tras cada página a atender las interrupciones; 256 bytes por la serie a 115200 baudios, unos 22 ms (provisional, hasta
/// la consola como servicio).
/// Un objeto de memoria, hasta 16 MB (una pantalla de 2560 × 1440 a 32 bits): crearlo tiene puntos de corte cada
/// 256 KB (S2), así que el tamaño no alarga lo que se pasa con las IRQ enmascaradas. La de DMA, 4 MB seguidos.
const MAX_MEMORIA: u64 = 16 << 20;
/// Lo más que mide un objeto de memoria disperso (llamada 57): 1 GB (2 MB de lista de páginas).
const MAX_DISPERSA: u64 = 1 << 30;
const MAX_DMA: u64 = 4 << 20;
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
    let (numero, a) = marco.llamada();
    let r = match numero {
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
        // A14: la hora del calendario en nanosegundos desde 1970 (UTC); 0 si no se sabe. a[0] = 0.
        // S13: con a[0] = 1, ponerla: el permiso del reloj en a[1] y la hora (ns desde 1970, UTC) en a[2].
        39 => match a[0] {
            0 => crate::reloj::hora(),
            1 => poner_hora(a[1] as u32, a[2]),
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
        // La caja Linux (H9): máquinas virtuales. En x86 (X10), con AMD-V o VT-x y la misma interfaz.
        45 => maquina_crear(a[0] as u32, a[1] as u32, a[2], a[3], a[4]),
        46 => maquina_correr(a[0] as u32, a[1], a[2], a[3]),
        47 => maquina_memoria(a[0] as u32, a[1] as u32, a[2]),
        48 => maquina_estado(a[0] as u32, a[1], a[2], a[3], a[4]),
        // H4 (S3): la página 0 de la caja Amiga, con su permiso en a[0] y el objeto de memoria (una página) en a[1].
        49 => pagina_cero(a[0] as u32, a[1] as u32),
        50 => memoria_vista(a[0], a[1], a[2], a[3]),
        // Apagar (a[1] = 0) o reiniciar (1) la máquina, con el permiso de apagar en a[0] (solo lo tiene `sesiones`).
        51 => apagar(a[0] as u32, a[1]),
        // Trasfondo: el tiempo de CPU de un grupo (con su llave, con leer) o, con 0, del grupo de quien llama.
        52 => cpu_tiempo(a[0] as u32),
        // Cupos como topes (S3): el aviso de memoria baja a un canal, con el permiso de apagar (solo `sesiones`).
        53 => memoria_aviso(a[0] as u32, a[1] as u32),
        // S3: cambiar el tope de un grupo (con su llave, con escribir; no el propio ni los de encima).
        54 => grupo_tope(a[0] as u32, a[1]),
        // Aviso a las cajas: el globo de una máquina (a[0] una vCPU con escribir; a[1] 0 soltar, 1 reponer; a[2] tabla
        // de direcciones del invitado en u64; a[3] cuántas, hasta 256).
        55 => maquina_globo(a[0] as u32, a[1], a[2], a[3]),
        // Qué es cada llave con que nació quien llama: sus nombres (16 bytes cada uno, en el orden de las llaves) a
        // a[0], hasta a[1] bytes. Devuelve cuántas tienen nombre (0 si quien lo lanzó no los dio).
        56 => llaves_nombres(a[0], a[1]),
        // El `RAM:` de una sesión (docs/bitacoras/asignaciones.md): un objeto de memoria disperso (a[0] bytes), cobrado
        // al grupo de la llave a[1] (0: el propio), y poner (a[1] = 1) o soltar (0) a[3] páginas suyas desde la a[2].
        57 => memoria_dispersa(a[0], a[1] as u32),
        58 => memoria_paginas(a[0] as u32, a[1], a[2], a[3]),
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
        // X6 (solo x86): los puertos de E/S de una llave de puertos, para el proceso que llama (era la 56 antes de unir
        // `revision`, donde la 56 es `llaves_nombres`).
        59 => puertos_permitir(a[0] as u32),
        // X11 (solo x86): pasar un aparato del PCIe a una máquina virtual (a[0] una vCPU, a[1] el aparato, a[2] la
        // operación). En ARM, `E_NO_HAY`.
        #[cfg(target_arch = "x86_64")]
        60 => maquina_aparato(marco, a[0] as u32, a[1] as u32, a[2], a[3], a[4]),
        #[cfg(not(target_arch = "x86_64"))]
        60 => E_NO_HAY,
        // S32 (solo x86): una variable del firmware (NVRAM), con el permiso de variables (a[0]), la operación (a[1]: 0
        // leer, 1 escribir, 2 la siguiente), el búfer (a[2]) y su capacidad (a[3]). Ver `firmware_variable`.
        #[cfg(target_arch = "x86_64")]
        64 => firmware_variable(marco, a[0] as u32, a[1], a[2], a[3]),
        #[cfg(not(target_arch = "x86_64"))]
        64 => E_NO_HAY,
        // K5 (S18): el hilo trata sus propios fallos. 61 pone su manejador (a[0] la entrada, 0 lo quita; a[1] y a[2] su
        // pila) y 62 vuelve de un fallo con el marco de a[0]. Solo en ARM; en x86, `E_NO_HAY` (aún no está).
        #[cfg(target_arch = "aarch64")]
        61 => if crate::excepciones::fallo_manejador(a[0], a[1], a[2]) { 0 } else { E_ARGUMENTO },
        #[cfg(target_arch = "aarch64")]
        62 => crate::excepciones::reanudar(marco, a[0]).unwrap_or(E_ARGUMENTO),
        #[cfg(not(target_arch = "aarch64"))]
        61 | 62 => E_NO_HAY,
        n => {
            kprintln!("  [hilo {}] llamada desconocida {}", planificador::actual_id(), n);
            E_ARGUMENTO
        }
    };
    marco.poner_x0(r);
    // Si lo libre para procesos ha cruzado el umbral del aviso de memoria baja (S3): dos lecturas si no.
    crate::presion::mirar();
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

/// Llamada 57: un objeto de memoria disperso de `tamano` bytes (hasta `MAX_DISPERSA`), sin ninguna página puesta, que
/// se cobra al grupo de la llave `grupo` (con escribir, como para crear un proceso en él; 0, el propio). Vacío solo
/// cuesta su lista de páginas. Las páginas se ponen y se sueltan con la llamada 58.
fn memoria_dispersa(tamano: u64, grupo: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if tamano == 0 || tamano > MAX_DISPERSA {
        return E_TAMANO;
    }
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
    if proc_.llaves.tomar().huecos() == 0 {
        return E_LIMITE;
    }
    let Some(m) = objetos::Memoria::dispersa(tamano as usize, &cupo) else { return E_MEMORIA };
    let llave = objetos::Llave::nueva(Objeto::Memoria(alloc::sync::Arc::new(m)), objetos::TODOS);
    match proc_.llaves.tomar().instalar(llave) {
        Ok(n) => n as u64,
        Err(_) => E_LIMITE,
    }
}

/// Llamada 58: pone (`op` 1) o suelta (0) `n` páginas (de 1 a 256) del objeto de memoria `h` (con escribir) desde la
/// `primera`, en las proyecciones de quien llama (`Espacio::paginas_de`). Devuelve cuántas quedaron como se pedía,
/// en orden desde la primera: menos de `n` si al poner no cupo más en el tope del grupo del objeto o no queda memoria.
fn memoria_paginas(h: u32, op: u64, primera: u64, n: u64) -> u64 {
    use crate::espacio::FalloPaginas;
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if op > 1 {
        return E_ARGUMENTO;
    }
    if n == 0 || n > crate::espacio::MAX_PAGINAS_SUELTAS as u64 || primera > u32::MAX as u64 {
        return E_TAMANO;
    }
    // Con la tabla de llaves y el espacio tomados (en ese orden, como el globo), nadie del proceso saca otra referencia
    // al objeto mientras se cuenta y se suelta. Una llave revocable no vale: su intermediario guarda otra referencia.
    let llaves = proc_.llaves.tomar();
    let Some(l) = llaves.ver_crudo(h) else { return E_LLAVE };
    let Objeto::Memoria(m) = &l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    // Las referencias de este proceso fuera de sus proyecciones: sus llaves y la copia de la llave que se acaba de ver.
    let mias = llaves.cuantas_de_memoria(m) + 1;
    let mut espacio = proc_.espacio.tomar();
    let r = espacio.paginas_de(m, primera as usize, n as usize, op == 1, mias);
    drop(espacio);
    drop(llaves);
    match r {
        Ok(k) => k as u64,
        Err(FalloPaginas::Fuera) => E_TAMANO,
        Err(FalloPaginas::Compartida) => E_DERECHOS,
        Err(FalloPaginas::Memoria) => E_MEMORIA,
    }
}

fn memoria_proyectar(h: u32, va: u64, permisos: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some((l, por)) = proc_.llaves.tomar().ver_con_intermediarios(h) else { return E_LLAVE };
    let Objeto::Memoria(m) = l.objeto else { return E_TIPO };
    let escribir = permisos & 2 != 0;
    // H4: el bit 2 es ejecutar (lo pide el JIT de la caja Amiga). Escribir y ejecutar por la misma dirección, nunca
    // (W^X): para eso se proyecta el mismo objeto dos veces.
    let ejecutar = permisos & 4 != 0;
    if escribir && ejecutar {
        return E_ARGUMENTO;
    }
    if escribir && l.derechos & objetos::ESCRIBIR == 0 || l.derechos & objetos::LEER == 0 {
        return E_DERECHOS;
    }
    let acceso = if ejecutar {
        crate::espacio::Acceso::Ejecutar
    } else if escribir {
        crate::espacio::Acceso::Escribir
    } else {
        crate::espacio::Acceso::Leer
    };
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
    if let Err(err) = proc_.espacio.tomar().proyectar_memoria(va, &m, acceso, &via) {
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

/// La llamada 45 (H4, S3): proyecta en la dirección 0, de solo lectura, un objeto de memoria de una página. Solo con el
/// permiso de página 0, que da el kernel a `inicio` y este a la caja Amiga: la caja guarda ahí el puntero a ExecBase
/// (dirección 4) y escribe el objeto por otra dirección. Una llave revocable de memoria no vale (la página 0 no se
/// apunta en intermediarios).
fn pagina_cero(permiso: u32, h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let m = {
        let t = proc_.llaves.tomar();
        let Some(p) = t.ver(permiso) else { return E_LLAVE };
        let Objeto::PermisoPaginaCero = p.objeto else { return E_TIPO };
        if p.derechos & objetos::ESCRIBIR == 0 {
            return E_DERECHOS;
        }
        let Some((l, por)) = t.ver_con_intermediarios(h) else { return E_LLAVE };
        let Objeto::Memoria(m) = l.objeto else { return E_TIPO };
        if !por.is_empty() {
            return E_ARGUMENTO;
        }
        if l.derechos & objetos::LEER == 0 {
            return E_DERECHOS;
        }
        m
    };
    if m.paginas() != 1 {
        return E_TAMANO;
    }
    match proc_.espacio.tomar().proyectar_pagina_cero(&m) {
        Ok(()) => 0,
        Err(e) => error_espacio(e),
    }
}

/// La llamada 39 con `x0` = 1 (S13): pone la hora del calendario (nanosegundos desde 1970, UTC). Solo con el permiso
/// del reloj, que da el kernel a `inicio` y este a `hora`, el servicio que la toma de la red. `E_ARGUMENTO` si la hora no
/// es creíble (anterior a 2024); entonces no cambia nada. El contador de la máquina (`ahora_us`, los plazos, los
/// temporizadores) no se toca: solo cambia la hora del calendario.
fn poner_hora(permiso: u32, ns: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    {
        let t = proc_.llaves.tomar();
        let Some(p) = t.ver(permiso) else { return E_LLAVE };
        let Objeto::PermisoReloj = p.objeto else { return E_TIPO };
        if p.derechos & objetos::ESCRIBIR == 0 {
            return E_DERECHOS;
        }
    }
    if crate::reloj::poner_hora_ns(ns) { 0 } else { E_ARGUMENTO }
}

/// La llamada 51 (decisión del usuario del 2026-11-13): apaga (`modo` 0) o reinicia (1) la máquina. Solo con el
/// permiso de apagar, que da el kernel a `inicio` y este a `sesiones`: el panel se lo pide a `sesiones`, que pregunta
/// antes si hay otras sesiones abiertas. Si se puede, no vuelve; si no (sin PSCI ni perro guardián), `E_NO_HAY`.
fn apagar(permiso: u32, modo: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    {
        let t = proc_.llaves.tomar();
        let Some(p) = t.ver(permiso) else { return E_LLAVE };
        let Objeto::PermisoApagar = p.objeto else { return E_TIPO };
        if p.derechos & objetos::ESCRIBIR == 0 {
            return E_DERECHOS;
        }
    }
    let reiniciar = match modo {
        0 => false,
        1 => true,
        _ => return E_ARGUMENTO,
    };
    kprintln!("kernel: {} la máquina (llamada 51)", if reiniciar { "se reinicia" } else { "se apaga" });
    // Un rato para que la consola saque lo último del registro (esto y lo de quien lo pidió) por el puerto serie.
    planificador::dormir_us(300_000);
    crate::nucleos::apagar(reiniciar);
    kprintln!("kernel: no se pudo {} la máquina", if reiniciar { "reiniciar" } else { "apagar" });
    E_NO_HAY
}

/// Orígenes de una vista, como mucho (la RAM de un invitado de 1 GB en trozos de 16 MB).
const MAX_ORIGENES: u64 = 64;

/// La llamada 50: una vista, un objeto de memoria hecho con páginas de otros (`origenes`, una tabla de llaves de
/// memoria con derecho de leer) sin copiarlas. `paginas` es una tabla de números de página de 32 bits que cuentan
/// seguidas las páginas de los orígenes en su orden; en el orden de bytes del proceso, como las de llaves. La llave
/// nueva solo puede leer y transferir; la vista no se proyecta para escribir ni ejecutar, ni vale como RAM de una
/// máquina. Mantiene vivos sus orígenes. Para que el compositor lea sin copia los búferes de un invitado (H9).
fn memoria_vista(origenes: u64, n_origenes: u64, paginas: u64, n_paginas: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if n_origenes == 0 || n_origenes > MAX_ORIGENES || n_paginas == 0 || n_paginas > MAX_MEMORIA / crate::memoria::PAGINA {
        return E_TAMANO;
    }
    let (Some(tabla), Some(lista)) = (leer_usuario(origenes, 4 * n_origenes as usize), leer_usuario(paginas, 4 * n_paginas as usize))
    else {
        return E_ARGUMENTO;
    };
    let numeros = llaves_de_bytes(&tabla, proc_.big_endian);
    let paginas = llaves_de_bytes(&lista, proc_.big_endian);
    let mut fuentes = Vec::new();
    if fuentes.try_reserve_exact(numeros.len()).is_err() {
        return E_MEMORIA;
    }
    {
        let t = proc_.llaves.tomar();
        for &n in &numeros {
            let Some((l, por)) = t.ver_con_intermediarios(n) else { return E_LLAVE };
            // Una llave revocable no vale: la vista seguiría viendo el objeto después de revocarla.
            if !por.is_empty() {
                return E_ARGUMENTO;
            }
            let Objeto::Memoria(m) = l.objeto else { return E_TIPO };
            if l.derechos & objetos::LEER == 0 {
                return E_DERECHOS;
            }
            fuentes.push(m);
        }
        if t.huecos() == 0 {
            return E_LIMITE;
        }
    }
    let m = match objetos::Memoria::vista(&fuentes, &paginas, &proc_.cupo) {
        Ok(m) => m,
        Err(crate::memoria::FalloVista::Memoria) => return E_MEMORIA,
        Err(_) => return E_ARGUMENTO,
    };
    let llave = objetos::Llave::nueva(Objeto::Memoria(alloc::sync::Arc::new(m)), objetos::LEER | objetos::TRANSFERIR);
    match proc_.llaves.tomar().instalar(llave) {
        Ok(n) => n as u64,
        Err(_) => E_LIMITE,
    }
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

/// Da al proceso los puertos de E/S de una llave de puertos (X6, solo x86): desde aquí los usa con `in` y `out`. Pide
/// leer y escribir. Vale para todos sus hilos; en los que corren ahora en otro núcleo, desde su próximo cambio. En ARM
/// no hay llaves de puertos: siempre `E_TIPO`.
fn puertos_permitir(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Puertos { primero, cuantos } = l.objeto else { return E_TIPO };
    if l.derechos & (objetos::LEER | objetos::ESCRIBIR) != objetos::LEER | objetos::ESCRIBIR {
        return E_DERECHOS;
    }
    #[cfg(target_arch = "x86_64")]
    {
        if !proc_.puertos.permitir(primero, cuantos) {
            return E_ARGUMENTO;
        }
        crate::excepciones::cargar_puertos(Some(&proc_.puertos));
        0
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (primero, cuantos);
        E_TIPO
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

/// En `n_llaves` de `proceso_crear`: tras la tabla de llaves van sus nombres (`NOMBRE_LLAVE` bytes cada uno).
const CON_NOMBRES: u64 = 1 << 16;

/// Los nombres de las llaves con que nació quien llama (los que dio quien lo lanzó): copia los que quepan enteros en
/// `capacidad` bytes y devuelve cuántos hay. El programa no los puede cambiar.
fn llaves_nombres(bufer: u64, capacidad: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let datos = proc_.nombres.datos();
    let caben = (capacidad as usize).min(datos.len()) / objetos::NOMBRE_LLAVE * objetos::NOMBRE_LLAVE;
    if caben > 0 && !escribir_usuario(bufer, &datos[..caben]) {
        return E_ARGUMENTO;
    }
    proc_.nombres.cuantos as u64
}

fn proceso_crear(h: u32, tamano: u64, llaves: u64, n_llaves: u64, grupo: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let con_nombres = n_llaves & CON_NOMBRES != 0;
    let n_llaves = n_llaves & !CON_NOMBRES;
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
    // Los nombres, antes de tocar nada: si no se pueden leer, no se crea nada y las llaves no se mueven.
    let nombres = if con_nombres {
        let Some(b) = llaves.checked_add(4 * n_llaves).and_then(|p| leer_usuario(p, objetos::NOMBRE_LLAVE * n_llaves as usize)) else { return E_ARGUMENTO };
        objetos::NombresLlaves::de(&b)
    } else {
        objetos::NombresLlaves::NINGUNO
    };
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
    match crate::procesos::lanzar_en_llamada(&binario, viajan, None, cupo, nombres) {
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

/// Un subgrupo del grupo propio con un tope de `tope` bytes (C42; desde S3, un tope y no una reserva: del propio solo
/// sale lo que ocupa el grupo en sí).
fn grupo_crear(tope: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if proc_.llaves.tomar().huecos() == 0 {
        return E_LIMITE;
    }
    let Some(g) = proc_.cupo.hijo(tope) else { return E_MEMORIA };
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
    marco.poner_x2(g.uso_propio());
    marco.poner_x3(g.topado());
    limite
}

/// Llamada 54 (S3): pone `tope` al grupo de `h` (con escribir). El propio grupo y los de encima, no (`E_DERECHOS`): un
/// proceso no se sube su tope. Por debajo de lo usado vale: no se quita nada y los cobros fallan hasta que baje.
fn grupo_tope(h: u32, tope: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Grupo(g) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 || proc_.cupo.es_o_cuelga_de(&g) {
        return E_DERECHOS;
    }
    g.poner_tope(tope);
    0
}

/// Llamada 53 (S3): el aviso de memoria baja, al otro lado del canal de `hc` (con escribir). Pide el permiso de memoria
/// (`permiso`, con escribir), que el kernel da a `inicio` y este solo a `sesiones`. Hay un solo oyente: el nuevo quita
/// al anterior. Nada más pedirlo llega el nivel de ahora.
fn memoria_aviso(permiso: u32, hc: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let (p, canal) = {
        let t = proc_.llaves.tomar();
        (t.ver(permiso), t.ver_con_intermediarios(hc))
    };
    let (Some(p), Some((canal, via))) = (p, canal) else { return E_LLAVE };
    let (Objeto::PermisoMemoria, Objeto::Extremo(e)) = (p.objeto, canal.objeto) else { return E_TIPO };
    if p.derechos & objetos::ESCRIBIR == 0 || canal.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    crate::presion::escuchar(e.canal().clone(), e.lado(), via.iter().map(|r| r.vigia()).collect(), canal.marca);
    0
}

/// Llamada 52 (trasfondo): microsegundos de CPU que han gastado los procesos del grupo `h` y de sus subgrupos, vivos y
/// muertos, desde que se creó. Con `h` = 0, los del grupo de quien llama (sin llave: es lo suyo). Solo con la llave del
/// grupo y el derecho de leer: nadie mide a otro sin que se la den (`sesiones` tiene las de lo que lanza).
fn cpu_tiempo(h: u32) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let cuentas = if h == 0 {
        proc_.cupo.cpu()
    } else {
        let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
        let Objeto::Grupo(g) = l.objeto else { return E_TIPO };
        if l.derechos & objetos::LEER == 0 {
            return E_DERECHOS;
        }
        g.cpu()
    };
    (cuentas as u128 * 1_000_000 / crate::reloj::frecuencia().max(1) as u128).min(i64::MAX as u128) as u64
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
    if tamano == 0 || tamano > MAX_DMA {
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
    // Las instrucciones de ARM van alineadas a 4; las de x86, a nada.
    if (cfg!(target_arch = "aarch64") && entrada % 4 != 0) || pila % 16 != 0 || pila < 16 {
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

/// Una máquina virtual (H9) con el permiso `hp` (escribir) y el objeto de memoria `hm` (leer y escribir; ni DMA ni
/// revocable) como su RAM, que el invitado ve desde `ipa`; empieza en `entrada`, en EL1, con la MMU apagada y `x0` en
/// su x0 (el árbol de dispositivos de un kernel de Linux). Lo que
/// cuestan su vCPU y sus tablas se cobra al grupo. Sin hipervisor, `E_NO_HAY`.
fn maquina_crear(hp: u32, hm: u32, ipa: u64, entrada: u64, x0: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let (permiso, memoria) = {
        let t = proc_.llaves.tomar();
        let Some(p) = t.ver(hp) else { return E_LLAVE };
        let Some(m) = t.ver_con_intermediarios(hm) else { return E_LLAVE };
        (p, m)
    };
    let Objeto::PermisoMaquina = permiso.objeto else { return E_TIPO };
    if permiso.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    let (m, por) = memoria;
    let derechos = m.derechos;
    // Revocar quita las proyecciones, no la etapa 2 de un invitado: una llave revocable no vale.
    if !por.is_empty() {
        return E_ARGUMENTO;
    }
    let Objeto::Memoria(m) = m.objeto else { return E_TIPO };
    let rw = objetos::LEER | objetos::ESCRIBIR;
    if derechos & rw != rw {
        return E_DERECHOS;
    }
    if !crate::base::hiper::hay() {
        return E_NO_HAY;
    }
    if proc_.llaves.tomar().huecos() == 0 {
        return E_LIMITE;
    }
    let maquina = match crate::maquinas::Maquina::nueva(m, ipa, entrada, x0, &proc_.cupo) {
        Ok(v) => v,
        Err(e) => return error_hiper(e),
    };
    let Ok(maquina) = alloc::sync::Arc::try_new(maquina) else { return E_MEMORIA };
    match proc_.llaves.tomar().instalar(objetos::Llave::nueva(Objeto::Maquina(maquina), objetos::TODOS)) {
        Ok(n) => n as u64,
        Err(_) => E_LIMITE,
    }
}

/// Corre la vCPU `h` (escribir) hasta que el invitado sale, y escribe el informe en `informe` (13 u64 en
/// little-endian, `maquinas::PALABRAS`). `valor` contesta a una lectura de aparato pendiente. `irq`, si no es 0, es el
/// bloque de interrupciones (`maquinas::IRQ_PALABRAS` u64): se lee antes de entrar y se escribe al volver. Devuelve el
/// motivo.
fn maquina_correr(h: u32, informe: u64, valor: u64, irq: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Maquina(m) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    // Antes de entrar, que se pueda escribir el informe: si no, lo que dijo el invitado se perdería.
    let vacio = [0u8; 8 * crate::maquinas::PALABRAS];
    if !escribir_usuario(informe, &vacio) {
        return E_ARGUMENTO;
    }
    let mut bloque = [0u64; crate::maquinas::IRQ_PALABRAS];
    if irq != 0 {
        let Some(b) = leer_usuario(irq, 8 * crate::maquinas::IRQ_PALABRAS) else { return E_ARGUMENTO };
        for (i, v) in bloque.iter_mut().enumerate() {
            *v = u64::from_le_bytes(b[8 * i..8 * i + 8].try_into().unwrap_or([0; 8]));
        }
    }
    match m.correr(valor, (irq != 0).then_some(&mut bloque)) {
        Ok(inf) => {
            let mut b = [0u8; 8 * crate::maquinas::PALABRAS];
            for (i, v) in inf.iter().enumerate() {
                b[8 * i..8 * i + 8].copy_from_slice(&v.to_le_bytes());
            }
            if !escribir_usuario(informe, &b) {
                return E_ARGUMENTO;
            }
            if irq != 0 {
                let mut b = [0u8; 8 * crate::maquinas::IRQ_PALABRAS];
                for (i, v) in bloque.iter().enumerate() {
                    b[8 * i..8 * i + 8].copy_from_slice(&v.to_le_bytes());
                }
                if !escribir_usuario(irq, &b) {
                    return E_ARGUMENTO;
                }
            }
            inf[0]
        }
        Err(crate::maquinas::Error::NoHay) => E_NO_HAY,
        Err(crate::maquinas::Error::Ocupada) => E_LIMITE,
        Err(crate::maquinas::Error::Muerta) => E_CERRADO,
    }
}

/// Operaciones sobre la vCPU `h` (escribir) y su máquina:
///   0 leer sus registros (x0..x30, PC y PSTATE: 33 u64 en little-endian) en `arg`, con ella parada;
///   1 escribirlos desde `arg` (PSTATE saneado: AArch64, EL0 o EL1), con ella parada;
///   2 un aborto externo para el invitado en lugar del último acceso a un aparato (`E_ARGUMENTO` si no lo hubo);
///   3 proyectar la GICV del GIC virtual en la dirección física del invitado `arg`, para todas sus vCPU (`E_NO_HAY` sin
///     GIC virtual);
///   4 otra vCPU de la misma máquina: la número `arg` (su Aff0 en el MPIDR, menos de 8 y sin otra con ese número), que
///     empieza en `a` con x0 = `b`; devuelve su llave (`E_ARGUMENTO` si el número no vale o ya está);
///   5 reiniciarla, parada, como recién encendida en `arg` con x0 = `a` (PSCI CPU_ON); en x86, en el modo `b` (0
///     protegido de 32 bits, 1 real: una SIPI);
///   6 patearla: que salga de su invitado en cuanto pueda (desde otro hilo; si no corre, la próxima vez no entra);
///   7 parar la máquina entera: todas sus vCPU salen y ya no entran (`E_CERRADO`).
fn maquina_estado(h: u32, op: u64, arg: u64, a: u64, b: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let Some(l) = proc_.llaves.tomar().ver(h) else { return E_LLAVE };
    let Objeto::Maquina(m) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    const N: usize = crate::base::hiper::REGISTROS;
    let error = |e| match e {
        crate::maquinas::Error::NoHay => E_NO_HAY,
        crate::maquinas::Error::Ocupada => E_LIMITE,
        crate::maquinas::Error::Muerta => E_CERRADO,
    };
    match op {
        0 => match m.registros() {
            Ok(r) => {
                let mut b = [0u8; 8 * N];
                for (i, v) in r.iter().enumerate() {
                    b[8 * i..8 * i + 8].copy_from_slice(&v.to_le_bytes());
                }
                if escribir_usuario(arg, &b) { 0 } else { E_ARGUMENTO }
            }
            Err(e) => error(e),
        },
        1 => {
            let Some(b) = leer_usuario(arg, 8 * N) else { return E_ARGUMENTO };
            let mut r = [0u64; N];
            for (i, v) in r.iter_mut().enumerate() {
                *v = u64::from_le_bytes(b[8 * i..8 * i + 8].try_into().unwrap_or([0; 8]));
            }
            match m.poner_registros(&r) {
                Ok(()) => 0,
                Err(e) => error(e),
            }
        }
        2 => match m.aborto() {
            Ok(true) => 0,
            Ok(false) => E_ARGUMENTO,
            Err(e) => error(e),
        },
        3 => match m.proyectar_gicv(arg, &proc_.cupo) {
            Ok(()) => 0,
            Err(e) => error_hiper(e),
        },
        4 => {
            if proc_.llaves.tomar().huecos() == 0 {
                return E_LIMITE;
            }
            let otra = match m.otra(arg, a, b, &proc_.cupo) {
                Ok(v) => v,
                Err(e) => return error_hiper(e),
            };
            let Ok(otra) = alloc::sync::Arc::try_new(otra) else { return E_MEMORIA };
            match proc_.llaves.tomar().instalar(objetos::Llave::nueva(Objeto::Maquina(otra), objetos::TODOS)) {
                Ok(n) => n as u64,
                Err(_) => E_LIMITE,
            }
        }
        #[cfg(target_arch = "aarch64")]
        5 => match m.reiniciar(arg, a) {
            Ok(()) => 0,
            Err(e) => error(e),
        },
        // x86 (X10): `b` es el modo en que empieza (0 protegido de 32 bits, 1 real, como tras una SIPI).
        #[cfg(target_arch = "x86_64")]
        5 => match m.reiniciar(arg, a, b) {
            Ok(()) => 0,
            Err(e) => error(e),
        },
        6 => {
            m.patear();
            0
        }
        7 => {
            m.parar();
            0
        }
        // S35, paso 2 (`docs/bitacoras/dominio-red.md`): prestar un aparato a un dominio de controladores. 40: los
        // registros de la llave `arg` (leer y escribir), en su misma dirección física. 41: el objeto de memoria DMA `arg`
        // (leer y escribir), en su dirección de bus. Sin IOMMU, solo para la prueba en QEMU. Solo en ARM: en x86 los
        // aparatos pasan a una máquina con la llamada 60 (X11), con IOMMU.
        #[cfg(target_arch = "aarch64")]
        40 | 41 => {
            let Some(r) = proc_.llaves.tomar().ver_con_intermediarios(arg as u32) else { return E_LLAVE };
            let (r, por) = r;
            // Una llave revocable no vale: revocarla no quitaría nada de la etapa 2.
            if !por.is_empty() {
                return E_ARGUMENTO;
            }
            if r.derechos & (objetos::LEER | objetos::ESCRIBIR) != objetos::LEER | objetos::ESCRIBIR {
                return E_DERECHOS;
            }
            let hecho = match (op, r.objeto) {
                (40, Objeto::Registros { fisica, tamano }) => m.prestar_registros(fisica, tamano, &proc_.cupo),
                (41, Objeto::Memoria(mem)) => m.anadir_dma(mem, &proc_.cupo),
                _ => return E_TIPO,
            };
            match hecho {
                Ok(()) => 0,
                Err(e) => error_hiper(e),
            }
        }
        _ => E_ARGUMENTO,
    }
}

/// Más RAM para el invitado de la máquina `h` (escribir): el objeto de memoria `hm` (leer y escribir; ni DMA ni
/// revocable), visto desde `ipa`, sin pisar lo que ya ve. Para un invitado de más de 16 MB, el tope de `memoria_crear`.
fn maquina_memoria(h: u32, hm: u32, ipa: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let (l, memoria) = {
        let t = proc_.llaves.tomar();
        let Some(l) = t.ver(h) else { return E_LLAVE };
        let Some(m) = t.ver_con_intermediarios(hm) else { return E_LLAVE };
        (l, m)
    };
    let Objeto::Maquina(maquina) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    let (m, por) = memoria;
    if !por.is_empty() {
        return E_ARGUMENTO;
    }
    let derechos = m.derechos;
    let Objeto::Memoria(m) = m.objeto else { return E_TIPO };
    let rw = objetos::LEER | objetos::ESCRIBIR;
    if derechos & rw != rw {
        return E_DERECHOS;
    }
    match maquina.anadir(m, ipa, &proc_.cupo) {
        Ok(()) => 0,
        Err(e) => error_hiper(e),
    }
}

/// Llamada 55 (`docs/bitacoras/aviso-cajas.md`): el globo de una máquina. Con la llave de una vCPU (escribir), suelta
/// (`op` 0) o repone (1) las páginas del invitado cuyas direcciones (u64, alineadas a 4 KB) están en la tabla `tabla`
/// (`n`, de 1 a 256). Solo memoria que tiene nadie más que la máquina y quien llama (ver `hiper::Espacio::globo`): las
/// páginas de otra se saltan. Devuelve cuántas se hicieron.
fn maquina_globo(h: u32, op: u64, tabla: u64, n: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    if n == 0 || n > crate::base::hiper::MAX_GLOBO as u64 {
        return E_TAMANO;
    }
    if op > 1 {
        return E_ARGUMENTO;
    }
    // La tabla se copia antes de tomar los cerrojos (copiar del proceso toma su espacio).
    let Some(bytes) = leer_usuario(tabla, 8 * n as usize) else { return E_ARGUMENTO };
    // En el orden de bytes del proceso, como las tablas de llaves.
    let ipas: Vec<u64> = bytes
        .chunks_exact(8)
        .map(|c| {
            let c: [u8; 8] = c.try_into().unwrap();
            if proc_.big_endian { u64::from_be_bytes(c) } else { u64::from_le_bytes(c) }
        })
        .collect();
    let llaves = proc_.llaves.tomar();
    let Some(l) = llaves.ver(h) else { return E_LLAVE };
    let Objeto::Maquina(maquina) = l.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    // Con la tabla de llaves y el espacio tomados, nadie del proceso puede sacar otra referencia al objeto mientras se
    // cuenta y se suelta (orden de los cerrojos: llaves, espacio y la etapa 2, como en el resto).
    let mut espacio = proc_.espacio.tomar();
    let cuenta = |m: &alloc::sync::Arc<crate::memoria::Memoria>| llaves.cuantas_de_memoria(m);
    let r = maquina.globo(&ipas, op == 0, &mut espacio, &cuenta);
    drop(espacio);
    drop(llaves);
    match r {
        Ok(k) => k as u64,
        Err(e) => error_hiper(e),
    }
}

/// X11 (solo x86, `pasar.rs`): el aparato del PCIe `ha` (una llave de aparato, con escribir) y la máquina de la vCPU
/// `h` (escribir):
///   0 asignárselo: FLR si lo admite, su DMA con la memoria del invitado (dirección = física del invitado) y su MSI en
///     el anfitrión. Devuelve la llave de su interrupción (0 si no tiene MSI ni MSI-X) y, en x1, las banderas
///     (`pasar::REMAPEADAS`, `pasar::CON_FLR`). `E_NO_HAY` sin IOMMU o sin hipervisor; `E_LIMITE` si ya está en una
///     máquina; `E_ARGUMENTO` si la máquina está parada;
///   1 proyectar su BAR `a` en la física del invitado `b` (alineada a 4 KB), sin las páginas de MSI-X;
///   2 la información de su BAR `a`: el tamaño, y en x1 las banderas (`pasar::BAR_*`);
///   3 leer su configuración: `a` el desplazamiento, `b` el tamaño (1, 2 o 4) → el valor;
///   4 escribirla: `a` el desplazamiento y en sus bits 23:16 el tamaño, `b` el valor. `E_DERECHOS` si no se deja;
///   5 (X12) una llave nueva de su interrupción `a` (la de la entrada `a` de su MSI-X; la 0 es la misma que da la 0),
///     y en x1 su número (el que llega en el mensaje de `interrupcion_asociar`). `E_ARGUMENTO` si no la tiene.
/// Las 1, 3, 4 y 5 piden el aparato ya en esa máquina (si no, `E_ARGUMENTO`). La 0 dice en los bits 15:8 de x1 cuántas
/// interrupciones tiene (`pasar::VECTORES_DESDE`).
#[cfg(target_arch = "x86_64")]
fn maquina_aparato(marco: &mut Marco, h: u32, ha: u32, op: u64, a: u64, b: u64) -> u64 {
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    let (l, la) = {
        let t = proc_.llaves.tomar();
        let Some(l) = t.ver(h) else { return E_LLAVE };
        let Some(la) = t.ver(ha) else { return E_LLAVE };
        (l, la)
    };
    let Objeto::Maquina(m) = l.objeto else { return E_TIPO };
    let Objeto::AparatoPci(ap) = la.objeto else { return E_TIPO };
    if l.derechos & objetos::ESCRIBIR == 0 || la.derechos & objetos::ESCRIBIR == 0 {
        return E_DERECHOS;
    }
    let cfg = |r: Result<u64, crate::pasar::ErrorCfg>| match r {
        Ok(v) => v,
        Err(crate::pasar::ErrorCfg::Argumento) => E_ARGUMENTO,
        Err(crate::pasar::ErrorCfg::Derechos) => E_DERECHOS,
    };
    match op {
        0 => {
            if !crate::base::hiper::hay() {
                return E_NO_HAY;
            }
            if proc_.llaves.tomar().huecos() == 0 {
                return E_LIMITE;
            }
            match m.asignar(&ap) {
                Ok((irq, banderas)) => {
                    marco.poner_x1(banderas);
                    match irq {
                        Some(i) => match proc_.llaves.tomar().instalar(objetos::Llave::nueva(Objeto::Interrupcion(i), objetos::TODOS)) {
                            Ok(n) => n as u64,
                            Err(_) => E_LIMITE,
                        },
                        None => 0,
                    }
                }
                Err(e) => error_hiper(e),
            }
        }
        1 => match m.proyectar_bar(&ap, a, b, &proc_.cupo) {
            Ok(()) => 0,
            Err(e) => error_hiper(e),
        },
        2 => match ap.info_bar(a as usize) {
            Some((tamano, banderas)) => {
                marco.poner_x1(banderas);
                tamano
            }
            None => E_ARGUMENTO,
        },
        3..=5 if !m.tiene(&ap) => E_ARGUMENTO,
        5 => match ap.interrupcion(a as usize) {
            Some((i, numero)) => {
                if proc_.llaves.tomar().huecos() == 0 {
                    return E_LIMITE;
                }
                match proc_.llaves.tomar().instalar(objetos::Llave::nueva(Objeto::Interrupcion(i), objetos::TODOS)) {
                    Ok(n) => {
                        marco.poner_x1(numero as u64);
                        n as u64
                    }
                    Err(_) => E_LIMITE,
                }
            }
            None => E_ARGUMENTO,
        },
        3 => cfg(ap.config_leer(a as usize, b as usize).map(|v| v as u64)),
        4 => cfg(ap.config_escribir((a & 0xfff) as usize, ((a >> 16) & 0xff) as usize, b as u32).map(|()| 0)),
        _ => E_ARGUMENTO,
    }
}

fn error_hiper(e: crate::base::hiper::Error) -> u64 {
    match e {
        crate::base::hiper::Error::NoHay => E_NO_HAY,
        crate::base::hiper::Error::Argumento => E_ARGUMENTO,
        crate::base::hiper::Error::Memoria => E_MEMORIA,
        crate::base::hiper::Error::Limite => E_LIMITE,
    }
}

/// La llamada 64 (S32, solo x86): una variable de la NVRAM de UEFI, por los servicios de ejecución del firmware
/// (`base::uefi`). Solo con el permiso de variables, que da el kernel a `inicio` y este al servicio `instalador` (solo
/// en vivo, desde el pendrive) y a nadie más.
///
/// El búfer: GUID (16 bytes), atributos (u32), largo del nombre en bytes (u32, UTF-16 con su 0 final), largo de los
/// datos (u32), 4 bytes sin usar, el nombre y los datos. Operaciones:
/// - 0, leer: los datos se escriben detrás del nombre (hasta la capacidad); x1 = sus bytes, y los atributos en su sitio.
/// - 1, escribir: solo las de arranque (`Boot####`, `BootOrder`, `BootNext`, del GUID global de EFI): es lo único que
///   necesita el instalador, y una variable cualquiera puede romper el firmware.
/// - 2, la siguiente tras (nombre, GUID): su nombre detrás del de antes (x1 = sus bytes) y su GUID en su sitio.
///
/// Devuelve el estado de UEFI (0 bien; con el bit 63, un error del firmware) o un `E_` del kernel.
#[cfg(target_arch = "x86_64")]
fn firmware_variable(marco: &mut Marco, permiso: u32, op: u64, buf: u64, capacidad: u64) -> u64 {
    use crate::base::uefi;
    const GLOBAL: [u8; 16] = [0x61, 0xDF, 0xE4, 0x8B, 0xCA, 0x93, 0xD2, 0x11, 0xAA, 0x0D, 0x00, 0xE0, 0x98, 0x03, 0x2B, 0x8C];
    let Some(proc_) = objetos::proceso_actual() else { return E_ARGUMENTO };
    {
        let t = proc_.llaves.tomar();
        let Some(p) = t.ver(permiso) else { return E_LLAVE };
        let Objeto::PermisoVariables = p.objeto else { return E_TIPO };
        if p.derechos & objetos::ESCRIBIR == 0 {
            return E_DERECHOS;
        }
    }
    if !(32..=64 * 1024).contains(&capacidad) || op > 2 {
        return E_ARGUMENTO;
    }
    let Some(cab) = leer_usuario(buf, 32) else { return E_ARGUMENTO };
    let mut guid = [0u8; 16];
    guid.copy_from_slice(&cab[..16]);
    let atr = u32::from_le_bytes(cab[16..20].try_into().unwrap());
    let (ln, ld) = (u32::from_le_bytes(cab[20..24].try_into().unwrap()) as u64, u32::from_le_bytes(cab[24..28].try_into().unwrap()) as u64);
    if ln < 2 || ln % 2 != 0 || ln > 1024 || 32 + ln + ld > capacidad {
        return E_TAMANO;
    }
    let Some(n) = leer_usuario(buf + 32, ln as usize) else { return E_ARGUMENTO };
    let nombre: Vec<u16> = n.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    if *nombre.last().unwrap() != 0 {
        return E_ARGUMENTO;
    }
    let sitio = (capacidad - 32 - ln) as usize;
    let (r, atr_leidos, bytes) = match op {
        0 => {
            let mut datos = alloc::vec![0u8; sitio];
            let r = uefi::variable(uefi::Operacion::Leer, &nombre, &mut guid, &mut datos);
            if r.0 == uefi::BIEN && !escribir_usuario(buf + 32 + ln, &datos[..r.2.min(sitio)]) {
                return E_ARGUMENTO;
            }
            r
        }
        1 => {
            let texto: alloc::string::String = char::decode_utf16(nombre[..nombre.len() - 1].iter().copied()).map(|c| c.unwrap_or('?')).collect();
            let de_arranque = texto == "BootOrder" || texto == "BootNext" || (texto.len() == 8 && texto.starts_with("Boot") && texto[4..].bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)));
            if guid != GLOBAL || !de_arranque || ld > 4096 {
                return E_DERECHOS;
            }
            let Some(datos) = leer_usuario(buf + 32 + ln, ld as usize) else { return E_ARGUMENTO };
            kprintln!("uefi: el instalador escribe la variable {} ({} bytes)", texto, ld);
            uefi::variable(uefi::Operacion::Escribir(atr, &datos), &nombre, &mut guid, &mut [])
        }
        _ => {
            let mut datos = alloc::vec![0u8; sitio.min(1024)];
            let r = uefi::variable(uefi::Operacion::Siguiente, &nombre, &mut guid, &mut datos);
            if r.0 == uefi::BIEN && (!escribir_usuario(buf + 32 + ln, &datos[..r.2.min(datos.len())]) || !escribir_usuario(buf, &guid)) {
                return E_ARGUMENTO;
            }
            r
        }
    };
    if op == 0 && r == uefi::BIEN {
        let _ = escribir_usuario(buf + 16, &atr_leidos.to_le_bytes());
    }
    let _ = atr;
    marco.poner_x1(bytes as u64);
    r
}
