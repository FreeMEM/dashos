//! El programa de inicio (C47). Lo lanza el kernel con las llaves de arranque: la 1 es el paquete de arranque, que
//! dice qué es cada una de las demás y trae los ELF de los servicios (formato en `kernel/src/inicio.rs`). Lanza cada
//! servicio en su grupo, con copias de las llaves de sus aparatos y los extremos de los canales que comparte con otros
//! (C50), y se queda esperando: no acaba nunca. En cada canal, el primer servicio que lo nombra es el servidor; los
//! demás reciben una copia marcada del otro extremo, y el servidor un mensaje sin marca por cliente: 8 bytes de marca y
//! 16 de nombre (auditoría de la fase 4, M6).

#![no_std]
#![no_main]

use dashos::{canal_crear, grupo_crear, llave_duplicar, UNA_VEZ, memoria_crear, memoria_proyectar, println, proceso_crear_en, proceso_esperar};

dashos::programa!(principal);

const PAQUETE: usize = 0x10_0000_0000;
const COPIA: usize = 0x20_0000_0000;

fn u32_en(p: usize) -> u32 {
    unsafe { core::ptr::read_volatile(p as *const u32) }
}

/// Las fichas del servicio de archivos (R6 de `docs/REVISION_LOOP.md`, U1 de S6b), provisionales hasta que reparta
/// `sesiones`: usuario, grupo, derechos (1: administrar volúmenes, instantáneas y comprobación) y cajón raíz de cada
/// programa que lo usa. Sin ficha no hay sesión. Las pruebas son del usuario 1000, dueño de todo el disco de pruebas
/// (`tools/disco_archivos.py`); `hostil`, de otro, encerrado en `C`.
const FICHAS: &[(&str, u32, u32, u32, &str)] = &[
    ("ficheros", 1000, 1000, 1, ""),
    ("medir-fs", 1000, 1000, 1, ""),
    ("imagen", 1000, 1000, 0, ""),
    ("hostil", 1001, 1001, 0, "C"),
];
/// La orden de alta del servicio de archivos.
const ALTA: u32 = 100;

/// Usuario y grupo de cada programa para `sesiones` (U2), provisionales como `FICHAS`: los de `FICHAS` y `presa`, del
/// mismo usuario que `hostil`. Los demás, servicios del sistema: el usuario 1 (ninguno corre como 0, S6b).
fn usuario_de(programa: &str) -> (u32, u32) {
    if let Some(&(_, uid, gid, _, _)) = FICHAS.iter().find(|f| f.0 == programa) {
        return (uid, gid);
    }
    if programa == "presa" { (1001, 1001) } else { (1, 1) }
}

/// Da de alta en `sesiones` (por `canal`, sin marca) al programa `nombre_cab` con la marca `marca` y una copia de la
/// llave de su grupo `grupo`, para que pueda matarlo entero.
fn alta_en_sesiones(canal: u32, nombre_cab: usize, marca: u64, grupo: u32) {
    let copia = llave_duplicar(grupo, 7, 0);
    if copia <= 0 {
        return;
    }
    let (uid, gid) = usuario_de(nombre(nombre_cab));
    let mut m = [0u8; 36];
    m[0..4].copy_from_slice(&1u32.to_le_bytes());
    m[4..12].copy_from_slice(&marca.to_le_bytes());
    m[12..16].copy_from_slice(&uid.to_le_bytes());
    m[16..20].copy_from_slice(&gid.to_le_bytes());
    m[20..36].copy_from_slice(unsafe { core::slice::from_raw_parts(nombre_cab as *const u8, 16) });
    for _ in 0..100 {
        if dashos::canal_escribir(canal, &m, &[copia as u32]) == 0 {
            return;
        }
        dormir_un_tic();
    }
    dashos::cerrar(copia as u32);
}

/// La baja en `sesiones` cuando acabe el proceso (U2, con el aviso de fin de K7): el kernel escribe su fin por
/// `canal` con la marca de la ficha y el bit alto puesto, que ningún programa puede poner en sus mensajes.
fn baja_en_sesiones(canal: u32, proceso: u32, marca: u64) {
    let aviso = llave_duplicar(canal, dashos::ESCRIBIR, (1 << 63) | marca);
    if aviso > 0 {
        dashos::proceso_avisar(proceso, aviso as u32);
        // El kernel guarda el canal y la marca: la llave ya no hace falta.
        dashos::cerrar(aviso as u32);
    }
}

/// El alta de `programa`, con la marca `marca`, para el servicio de archivos: cabecera de 32 bytes y el cajón raíz.
fn alta(programa: &str, marca: u64) -> Option<([u8; 96], usize)> {
    let &(_, uid, gid, derechos, raiz) = FICHAS.iter().find(|f| f.0 == programa)?;
    let mut m = [0u8; 96];
    m[0..4].copy_from_slice(&ALTA.to_le_bytes());
    m[4..8].copy_from_slice(&derechos.to_le_bytes());
    m[8..16].copy_from_slice(&marca.to_le_bytes());
    m[16..20].copy_from_slice(&uid.to_le_bytes());
    m[20..24].copy_from_slice(&gid.to_le_bytes());
    m[24..28].copy_from_slice(&(raiz.len() as u32).to_le_bytes());
    m[32..32 + raiz.len()].copy_from_slice(raiz.as_bytes());
    Some((m, 32 + raiz.len()))
}

fn nombre(p: usize) -> &'static str {
    let b = unsafe { core::slice::from_raw_parts(p as *const u8, 16) };
    core::str::from_utf8(&b[..b.iter().position(|&c| c == 0).unwrap_or(16)]).unwrap_or("?")
}

/// Espera un tic sin gastar CPU (un temporizador de una vez).
fn dormir_un_tic() {
    let (a, b) = canal_crear();
    let t = dashos::temporizador_crear(b, 10_000, 0);
    if t > 0 {
        let mut buf = [0u8; 8];
        let mut ll = [0u32; 1];
        dashos::canal_leer(a, &mut buf, &mut ll);
        dashos::cerrar(t as u32);
    }
    dashos::cerrar(a);
    dashos::cerrar(b);
}

fn principal() -> u64 {
    if memoria_proyectar(1, PAQUETE, false) != 0 {
        println!("  [inicio] sin paquete de arranque");
        return 1;
    }
    let mut p = PAQUETE;
    let n = u32_en(p) as usize;
    p += 4;
    let aparatos = p;
    println!("  [inicio] en marcha, con {} llaves de aparatos:", n);
    for _ in 0..n {
        let tipo = match u32_en(p + 16) {
            1 => "registros",
            2 => "interrupción",
            3 => "permiso DMA",
            4 => "registro",
            5 => "memoria del firmware",
            6 => "datos",
            _ => "?",
        };
        println!("  [inicio]   llave {}: {} de {}", u32_en(p + 20), tipo, nombre(p));
        p += 24;
    }
    let programas = u32_en(p) as usize;
    p += 4;
    let cabeceras = p;
    let mut datos = cabeceras + CABECERA * programas;
    // Los canales entre servicios: el extremo que reciben, en copias, los demás programas que lo nombran (0: aún no
    // creado). El primero recibe el otro extremo.
    let mut pendientes = [0u32; 32];
    // Quién lee cada canal (su cabecera), para saber cuál es el del servicio de archivos.
    let mut lector = [0usize; 32];
    // E6: los servicios (los que leen algún canal), para relanzarlos si acaban. Sus avisos de fin (K7) llegan por
    // `avisos`, con la marca del número del programa más 1.
    let mut servicios: [Option<Servicio>; 32] = Default::default();
    // Los clientes de cada canal (bit = número del programa), para darle otra vez a su servicio, si se relanza, sus
    // entradas del directorio y, al de archivos, sus altas.
    let mut clientes_de = [0u64; 32];
    // El canal (sin marca) de `sesiones`, en cuanto se lance (U2).
    let mut sesiones = 0u32;
    let (avisos_a, avisos) = canal_crear();
    for i in 0..programas {
        let cab = cabeceras + CABECERA * i;
        let tamano = u32_en(cab) as usize;
        let de_canales = u32_en(cab + 4);
        let nombre_cab = cab + 16;
        // Las llaves que le tocan, en orden; pasan a ser la 1, la 2... del servicio. De los aparatos, una copia: así
        // `inicio` conserva la suya y puede dar el mismo aparato a otro (el permiso DMA, por ejemplo). La copia llega
        // sin el derecho de transferir (S4): un controlador no puede pasar sus registros a otro programa.
        let mut suyas = [0u32; 8];
        let mut k = 0;
        for j in 0..8 {
            let a = unsafe { *((cab + 8 + j) as *const u8) } as usize;
            if a < n && k < suyas.len() {
                let copia = llave_duplicar(u32_en(aparatos + 24 * a + 20), 7 | UNA_VEZ, 0);
                if copia > 0 {
                    suyas[k] = copia as u32;
                    k += 1;
                }
            }
        }
        // Los canales que crea este programa: tras sus llaves, una copia para escribirse a sí mismo (paso B de
        // docs/MULTITAREA_LOOP.md: un temporizador que lo despierte en su propio canal), con una marca que no es de
        // ningún cliente.
        let mut propios = [0u32; 8];
        let mut np = 0;
        let mut lee = 0u32;
        for c in 0..32 {
            if de_canales & (1 << c) != 0 && k < suyas.len() {
                if pendientes[c] == 0 {
                    let (a, b) = canal_crear();
                    pendientes[c] = b;
                    lector[c] = cab;
                    lee |= 1 << c;
                    suyas[k] = a;
                    k += 1;
                    if np < propios.len() {
                        propios[np] = b;
                        np += 1;
                    }
                } else {
                    // Cada cliente, con su marca (el número del programa más 1), y el servidor sabe de quién es por un
                    // mensaje sin marca que solo puede mandar `inicio` (auditoría de la fase 4, M6).
                    let marca = i as u64 + 1;
                    let copia = llave_duplicar(pendientes[c], 7, marca);
                    if copia > 0 {
                        suyas[k] = copia as u32;
                        k += 1;
                        let mut dir = [0u8; 24];
                        dir[..8].copy_from_slice(&marca.to_le_bytes());
                        dir[8..].copy_from_slice(unsafe { core::slice::from_raw_parts(nombre_cab as *const u8, 16) });
                        // Si la cola del servidor está llena, se reintenta un rato: sin su entrada, el cliente no
                        // podría pedir lo suyo (segunda vuelta de la auditoría, B16).
                        for _ in 0..100 {
                            if dashos::canal_escribir(pendientes[c], &dir, &[]) == 0 {
                                break;
                            }
                            dormir_un_tic();
                        }
                        if i < 64 {
                            clientes_de[c] |= 1 << i;
                        }
                        // Al de archivos, además, su ficha (R6), también antes de que el cliente arranque.
                        if nombre(lector[c] + 16) == "archivos" {
                            if let Some((m, n)) = alta(nombre(nombre_cab), marca) {
                                for _ in 0..100 {
                                    if dashos::canal_escribir(pendientes[c], &m[..n], &[]) == 0 {
                                        break;
                                    }
                                    dormir_un_tic();
                                }

                            }
                        }
                    }
                }
            }
        }
        for &b in &propios[..np] {
            if k < suyas.len() {
                let copia = llave_duplicar(b, dashos::ESCRIBIR | dashos::TRANSFERIR, u64::MAX);
                if copia > 0 {
                    suyas[k] = copia as u32;
                    k += 1;
                }
            }
        }
        datos = (datos + 7) & !7;
        let origen = datos;
        datos += tamano;
        let m = memoria_crear(tamano);
        let destino = COPIA + i * (8 << 20);
        if (m as i32) <= 0 || memoria_proyectar(m, destino, true) != 0 {
            println!("  [inicio] {}: sin memoria", nombre(nombre_cab));
            continue;
        }
        unsafe { core::ptr::copy_nonoverlapping(origen as *const u8, destino as *mut u8, tamano) };
        // 16 MB por servicio; el de archivos, 64, para su caché de bloques (paso F9 de docs/MULTITAREA_LOOP.md: con 6 MB
        // de caché, un fichero de 8 MB leído de corrido iba siempre al disco).
        let cupo = if nombre(nombre_cab) == "archivos" { 64 << 20 } else { 16 << 20 };
        // De un servicio, inicio guarda una copia de cada llave, con los mismos derechos, para relanzarlo (E6).
        let mut guardadas = [0u32; 8];
        if lee != 0 {
            for j in 0..k {
                guardadas[j] = llave_duplicar(suyas[j], u64::MAX, 0).max(0) as u32;
            }
        }
        let g = grupo_crear(cupo);
        let proceso = if g > 0 { proceso_crear_en(g as u32, m, tamano, &suyas[..k]) } else { g };
        if proceso <= 0 {
            println!("  [inicio] {}: no se pudo lanzar ({})", nombre(nombre_cab), proceso);
            continue;
        }
        println!("  [inicio] {} lanzado en su grupo, con {} llaves", nombre(nombre_cab), k);
        // U2: `sesiones` lee su canal; desde ese momento, cada programa que se lanza tiene su ficha allí.
        if nombre(nombre_cab) == "sesiones" {
            if let Some(c) = (0..32).find(|&c| lee & (1 << c) != 0) {
                sesiones = pendientes[c];
            }
        } else if sesiones != 0 {
            alta_en_sesiones(sesiones, nombre_cab, i as u64 + 1, g as u32);
            baja_en_sesiones(sesiones, proceso as u32, i as u64 + 1);
        }
        if lee != 0 && i < servicios.len() {
            let aviso = llave_duplicar(avisos_a, dashos::ESCRIBIR, i as u64 + 1).max(0) as u32;
            if aviso > 0 && dashos::proceso_avisar(proceso as u32, aviso) == 0 {
                servicios[i] = Some(Servicio { cab: nombre_cab, m, tamano, guardadas, k, cupo, lee, aviso, proceso: proceso as u32, grupo: g as u32, veces: 0 });
            }
        }
        // Los servicios de verdad (consola, USB) no acaban; los de prueba, sí: a esos se les espera.
        if k == 0 {
            let (clase, codigo) = proceso_esperar(proceso as u32);
            println!("  [inicio] {} acabó: clase {}, código {}", nombre(nombre_cab), clase, codigo);
        }
    }
    println!("  [inicio] todo lanzado; se queda vigilando los servicios");
    // E6: un servicio que acaba se relanza con las mismas llaves, tras 100, 200 y 400 ms; a la cuarta, se deja. Su
    // canal sigue siendo el mismo (inicio guarda una copia de su extremo): sus clientes no lo pierden.
    let mut buf = [0u8; 16];
    loop {
        let (n, k, marca) = dashos::canal_leer_marca(avisos, &mut buf, &mut []);
        if n != 16 || k != 0 || marca == 0 || marca as usize > servicios.len() {
            continue;
        }
        let i = marca as usize - 1;
        let Some(s) = servicios[i].as_mut() else { continue };
        let clase = u64::from_le_bytes(buf[..8].try_into().unwrap());
        let codigo = u64::from_le_bytes(buf[8..].try_into().unwrap());
        dashos::cerrar(s.proceso);
        dashos::cerrar(s.grupo);
        if s.veces >= MAX_RELANZAR {
            println!("  [inicio] {} acabó (clase {}, código {}): se deja tras {} relanzados", nombre(s.cab), clase, codigo, s.veces);
            servicios[i] = None;
            continue;
        }
        s.veces += 1;
        println!("  [inicio] {} acabó (clase {}, código {}): se relanza (vez {})", nombre(s.cab), clase, codigo, s.veces);
        dashos::dormir_us(100_000 << (s.veces - 1));
        if !relanzar(s, &pendientes, &clientes_de, &lector, cabeceras) {
            println!("  [inicio] {}: no se pudo relanzar", nombre(s.cab));
            servicios[i] = None;
        }
    }
}

/// Un servicio vigilado (E6): de dónde sale y con qué llaves, para relanzarlo.
struct Servicio {
    cab: usize,
    m: u32,
    tamano: usize,
    guardadas: [u32; 8],
    k: usize,
    cupo: u64,
    /// Los canales que lee (bits).
    lee: u32,
    /// La copia marcada del canal de avisos de fin.
    aviso: u32,
    proceso: u32,
    grupo: u32,
    veces: u32,
}

const MAX_RELANZAR: u32 = 3;
/// Lo que mide la cabecera de cada programa en el paquete de arranque.
const CABECERA: usize = 32;

/// Relanza `s` con copias de sus llaves guardadas, en un grupo nuevo, y pide otra vez su aviso de fin. Antes deja en
/// cada canal que lee un aviso sin marca de que es un relanzado y, otra vez, lo que se le dio de sus clientes (su
/// entrada del directorio y, al de archivos, su alta): sin eso no sabría quién le habla.
fn relanzar(s: &mut Servicio, pendientes: &[u32; 32], clientes_de: &[u64; 32], lector: &[usize; 32], cabeceras: usize) -> bool {
    let escribir = |p: u32, m: &[u8]| {
        for _ in 0..100 {
            if dashos::canal_escribir(p, m, &[]) == 0 {
                break;
            }
            dormir_un_tic();
        }
    };
    for (c, &p) in pendientes.iter().enumerate() {
        if s.lee & (1 << c) != 0 && p != 0 {
            escribir(p, b"inicio: relanzado");
            for j in (0..64).filter(|j| clientes_de[c] & (1 << j) != 0) {
                // Como al lanzarlos: marca (el número del programa más 1) y nombre; al de archivos, también la ficha.
                let marca = j as u64 + 1;
                let nombre_cab = cabeceras + CABECERA * j + 16;
                let mut dir = [0u8; 24];
                dir[..8].copy_from_slice(&marca.to_le_bytes());
                dir[8..].copy_from_slice(unsafe { core::slice::from_raw_parts(nombre_cab as *const u8, 16) });
                escribir(p, &dir);
                if nombre(lector[c] + 16) == "archivos" {
                    if let Some((m, n)) = alta(nombre(nombre_cab), marca) {
                        escribir(p, &m[..n]);
                    }
                }
            }
        }
    }
    let mut suyas = [0u32; 8];
    for j in 0..s.k {
        let c = llave_duplicar(s.guardadas[j], u64::MAX, 0);
        if c <= 0 {
            return false;
        }
        suyas[j] = c as u32;
    }
    let g = grupo_crear(s.cupo);
    let proceso = if g > 0 { proceso_crear_en(g as u32, s.m, s.tamano, &suyas[..s.k]) } else { g };
    if proceso <= 0 {
        return false;
    }
    s.proceso = proceso as u32;
    s.grupo = g as u32;
    dashos::proceso_avisar(s.proceso, s.aviso) == 0
}
