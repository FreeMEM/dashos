//! El programa de inicio (C47). Lo lanza el kernel con las llaves de arranque: la 1 es el paquete de arranque, que
//! dice qué es cada una de las demás y trae los ELF de los servicios (formato en `kernel/src/inicio.rs`). Lanza cada
//! servicio en su grupo, con copias de las llaves de sus aparatos y los extremos de los canales que comparte con otros
//! (C50), y se queda esperando: no acaba nunca. En cada canal, el primer servicio que lo nombra es el servidor; los
//! demás reciben una copia marcada del otro extremo, y el servidor un mensaje sin marca por cliente: 8 bytes de marca y
//! 16 de nombre (auditoría de la fase 4, M6).
//!
//! A cada programa le dice qué es cada llave que le da (la tabla de llaves, `usuario/src/llaves.rs`): un aparato se
//! llama como en el paquete; un canal, como el servicio ("ventanas", "archivos"...), tanto para quien lo sirve como
//! para sus clientes; la copia para escribirse a sí mismo, "yo:" y el canal; y la copia sin marca del canal de
//! archivos que recibe `sesiones`, "archivos.altas".

#![no_std]
#![no_main]

use dashos::{canal_crear, grupo_crear, llave_duplicar, UNA_VEZ, memoria_crear, memoria_proyectar, println, proceso_crear_con_nombres, proceso_esperar};

dashos::programa!(principal);

const PAQUETE: usize = 0x10_0000_0000;
const COPIA: usize = 0x20_0000_0000;
/// Donde se copian las cabeceras del paquete antes de soltarlo (antes de ponerlas en `PAQUETE`).
const COPIA_CABECERAS: usize = 0x30_0000_0000;
/// Donde se mira un momento el objeto "sistema" (la RAM de la máquina).
const SISTEMA: usize = 0x38_0000_0000;

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
    // U3: `sesiones` abre y cierra el volumen de cada usuario (administrar volúmenes); no lee lo de nadie.
    ("sesiones", 1, 1, 1, ""),
    // U5: el selector de prueba, del usuario 1000, con el derecho de repartir (ceder asas).
    ("selector", 1000, 1000, 2, ""),
    // S10c: el centro de control, para ver cuánto ocupa el disco (INFO): sin derechos y encerrado en `C`.
    ("centro", 1000, 1000, 0, "C"),
    // S10: la barra, para listar los programas de su lanzador: del sistema, sin derechos y encerrada en `Programas`.
    // Lanzar no lo hace ella: lo hace `sesiones`, como la persona que está delante.
    ("barra", 1, 1, 0, "Programas"),
    // C3: el fondo de escritorio, para leer las imágenes de `C/Fondos` y su nombre en `C/Prefs/fondo`.
    ("fondo", 1000, 1000, 0, "C"),
    // S10: el Workbench, de Ana (1000), con el derecho de repartir, como el selector: al empezar se cede a sí mismo el
    // disco solo para leer (raíz 1, "Sistema") y `C` para leer y escribir (raíz 2, "Home" provisional), y solo va por
    // ellas.
    ("workbench", 1000, 1000, 2, ""),
    // El explorador de archivos, como el Workbench: de Ana, con el derecho de repartir; se cede sus raíces (Sistema
    // para leer, `C` como Home y el cajón de la caja Amiga) y solo va por ellas.
    ("explorador", 1000, 1000, 2, ""),
    // A15: el comprobador aparte, para sujetar el disco mientras lee la foto (administrar). No lee nada por archivos;
    // la raíz es la del disco porque no todos los discos tienen `C` (el de medir-fs no), y sin raíz no hay alta.
    ("comprobador", 1, 1, 1, ""),
    // H5: la caja Amiga, de Ana (1000). S28: su raíz 0 (leer y escribir) es su cajón, que la caja monta como `Caja:`;
    // el disco va aparte, solo para leer (`RAICES_DE_MAS`), y es su `SYS:`. Así el servicio niega escribir en el disco
    // aunque alguien se hiciera con la caja. Provisional, como todo `FICHAS`: con `sesiones`, su cajón será
    // `Home:Datos/<paquete>` (S6b).
    ("caja-amiga", 1000, 1000, 0, "Estado/Servicios/caja-amiga"),
    // kit-c: la prueba de las ventanas de Dash, de Ana, sin derechos y encerrada en su cajón: lee su catálogo de
    // `Locale/Catalogs` de ahí (LOCALE:, provisional hasta las asignaciones).
    ("dash-ventana", 1000, 1000, 0, "Estado/Servicios/dash-ventana"),
    // S13: el servicio de red, para leer lo que se configura en `Prefs/Red` del disco del sistema (`hosts` y `hora`):
    // del sistema, sin derechos y encerrado ahí.
    ("red", 1, 1, 0, "Prefs/Red"),
];
/// Raíces de más en el alta de archivos (U2, S6b capa 2): programa, hueco (1 a 15), tope (4 leer, 6 leer y escribir) y
/// camino desde la raíz del disco.
const RAICES_DE_MAS: &[(&str, u8, u8, &str)] = &[
    // S28: el disco entero, solo para leer: el `SYS:` de la caja Amiga.
    ("caja-amiga", 1, 4, ""),
    // S28 (decisión del usuario del 2026-11-13): la papelera de la caja Amiga, junto a su cajón y fuera de su alcance:
    // lo que borra va ahí (`HUECO_PAPELERA`). Provisional, como su cajón.
    ("caja-amiga", dashos::protocolo::archivos::HUECO_PAPELERA, 6, "Estado/Servicios/caja-amiga.papelera"),
<<<<<<< HEAD
    // S13: los ajustes del sistema, solo para leer, para la barra: de ahí saca la zona horaria (`Prefs/zona`).
    ("barra", 1, 4, "Prefs"),
=======
    // Idiomas (paso 4): el centro de control lee su idioma y sus catálogos: el disco (su `SYS:`, donde está `LOCALE:`)
    // y el `RAM:` de la sesión (donde está `ENV:`), los dos solo para leer. `sesiones` le pone esos dos volúmenes en
    // su tabla de nombres (`LEEN_IDIOMA`). Provisional, como `FICHAS`: el `RAM:` de Ana va fijo.
    ("centro", 1, 4, ""),
    ("centro", 3, 4, "Estado/RAM/Ana"),
>>>>>>> idiomas
];
/// Los servicios cuyas llaves reparte `sesiones` (U4): de ellos recibe una copia sin marca, para hacer copias marcadas
/// con el número de cada programa al que se las da. Del resto (archivos), una con la suya, como cualquier cliente.
const REPARTE: &[&str] = &["eco"];

/// Los servicios que siguen las sesiones (U6, paso 2): de su canal, `sesiones` recibe una copia sin marca para
/// avisarles de quién es cada programa y qué sesión está detrás, y para dar a lo que lanza su llave marcada.
/// S10: también `avisos`: lo de una sesión de detrás se guarda sin salir a la de delante.
const SIGUEN_SESIONES: &[&str] = &["compositor", "sonido", "avisos"];
/// Otros servicios de cuyo canal recibe `sesiones` una copia sin marca, igual que de los de arriba: el selector de
/// ficheros, para dar su llave a los programas que lanza y presentárselos (S6b: el selector es el camino por el que un
/// programa encerrado llega a un fichero que el usuario elige).
const A_SESIONES: &[&str] = &["selector"];
/// Si `sesiones` recibe una copia sin marca del canal que sirve `programa`.
fn va_a_sesiones(programa: &str) -> bool {
    SIGUEN_SESIONES.contains(&programa) || A_SESIONES.contains(&programa)
}

/// Programas de prueba que llevan alguna llave de aparato y que, aun así, se esperan (como los que no llevan ninguna):
/// la caja Amiga, con el permiso de página 0 (H4).
const SE_ESPERAN: &[&str] = &["caja-amiga"];

/// El permiso de apagar y reiniciar del kernel (tipo 10, "apagar"), si lo hay: se le manda a `sesiones` al lanzarlo y
/// al relanzarlo (no cabe entre sus 8 llaves).
static PERMISO_APAGAR: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// El permiso del aviso de memoria baja (tipo 11, "memoria", S3): a `sesiones`, como el de apagar.
static PERMISO_MEMORIA: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Manda a `sesiones` (por `canal`, sin marca) una copia del permiso de apagar y otra del de memoria, solo para escribir:
/// llegan sin el derecho de transferirlas (UNA_VEZ), así que no las puede pasar a nadie.
fn apagar_a_sesiones(canal: u32) {
    permiso_a_sesiones(canal, &PERMISO_APAGAR, dashos::protocolo::sesiones::PERMISO_APAGAR);
    permiso_a_sesiones(canal, &PERMISO_MEMORIA, dashos::protocolo::sesiones::PERMISO_MEMORIA);
}

fn permiso_a_sesiones(canal: u32, cual: &core::sync::atomic::AtomicU32, orden: u32) {
    let permiso = cual.load(core::sync::atomic::Ordering::Relaxed);
    if permiso == 0 {
        return;
    }
    let copia = llave_duplicar(permiso, dashos::ESCRIBIR | dashos::TRANSFERIR | UNA_VEZ, 0);
    if copia <= 0 {
        return;
    }
    let orden = orden.to_le_bytes();
    for _ in 0..500 {
        if dashos::canal_escribir(canal, &orden, &[copia as u32]) == 0 {
            return;
        }
        dormir_un_tic();
    }
    dashos::cerrar(copia as u32);
}

/// Manda a `sesiones` (por `canal`, sin marca) una copia sin marca del canal `extremo`, que sirve `nombre_cab`.
fn canal_a_sesiones(canal: u32, extremo: u32, nombre_cab: usize) {
    let copia = llave_duplicar(extremo, 7, 0);
    if copia <= 0 {
        return;
    }
    let mut m = [0u8; 20];
    m[0..4].copy_from_slice(&dashos::protocolo::sesiones::CANAL_SISTEMA.to_le_bytes());
    m[4..20].copy_from_slice(unsafe { core::slice::from_raw_parts(nombre_cab as *const u8, 16) });
    // Hasta 5 s: sin ella, las ventanas de una sesión de detrás se verían.
    for _ in 0..500 {
        if dashos::canal_escribir(canal, &m, &[copia as u32]) == 0 {
            return;
        }
        dormir_un_tic();
    }
    dashos::cerrar(copia as u32);
}

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
    m[0..4].copy_from_slice(&dashos::protocolo::sesiones::ALTA.to_le_bytes());
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
fn alta(programa: &str, marca: u64) -> Option<([u8; 160], usize)> {
    let &(_, uid, gid, derechos, raiz) = FICHAS.iter().find(|f| f.0 == programa)?;
    let mut m = [0u8; 160];
    m[0..4].copy_from_slice(&ALTA.to_le_bytes());
    m[4..8].copy_from_slice(&derechos.to_le_bytes());
    m[8..16].copy_from_slice(&marca.to_le_bytes());
    m[16..20].copy_from_slice(&uid.to_le_bytes());
    m[20..24].copy_from_slice(&gid.to_le_bytes());
    m[24..28].copy_from_slice(&(raiz.len() as u32).to_le_bytes());
    m[32..32 + raiz.len()].copy_from_slice(raiz.as_bytes());
    let mut n = 32 + raiz.len();
    // Las raíces de más: hueco, tope, largo del volumen (0: el principal), largo del camino y el camino.
    for &(_, hueco, tope, camino) in RAICES_DE_MAS.iter().filter(|r| r.0 == programa) {
        if n + 4 + camino.len() > m.len() {
            break;
        }
        m[n..n + 4].copy_from_slice(&[hueco, tope, 0, camino.len() as u8]);
        m[n + 4..n + 4 + camino.len()].copy_from_slice(camino.as_bytes());
        n += 4 + camino.len();
    }
    Some((m, n))
}

/// Si al programa de la cabecera `cab` le toca el aparato `quien` (por su nombre en la lista de `aparatos`, que tiene
/// `n`).
fn tiene_aparato(cab: usize, aparatos: usize, n: usize, quien: &str) -> bool {
    (0..8).any(|j| {
        let a = unsafe { *((cab + 8 + j) as *const u8) } as usize;
        a < n && nombre(aparatos + 24 * a) == quien
    })
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
            7 => "permiso de plazo",
            8 => "permiso de máquinas virtuales",
            9 => "permiso de página 0",
            10 => "permiso de apagar",
            11 => "permiso del aviso de memoria",
            12 => "permiso del reloj",
            _ => "?",
        };
        println!("  [inicio]   llave {}: {} de {}", u32_en(p + 20), tipo, nombre(p));
        p += 24;
    }
    if let Some(i) = (0..n).find(|&i| nombre(aparatos + 24 * i) == "apagar" && u32_en(aparatos + 24 * i + 16) == 10) {
        PERMISO_APAGAR.store(u32_en(aparatos + 24 * i + 20), core::sync::atomic::Ordering::Relaxed);
    }
    if let Some(i) = (0..n).find(|&i| nombre(aparatos + 24 * i) == "memoria" && u32_en(aparatos + 24 * i + 16) == 11) {
        PERMISO_MEMORIA.store(u32_en(aparatos + 24 * i + 20), core::sync::atomic::Ordering::Relaxed);
    }
    // Lo que dice el kernel de la máquina ("sistema"): la RAM, para la memoria de la caja Linux (H9).
    // Y lo pedido para el invitado al compilar (caja-linux-grandes), o 0.
    let (ram, pedida) = (0..n)
        .find(|&i| nombre(aparatos + 24 * i) == "sistema" && u32_en(aparatos + 24 * i + 16) == 6)
        .and_then(|i| {
            let llave = u32_en(aparatos + 24 * i + 20);
            if memoria_proyectar(llave, SISTEMA, false) != 0 {
                return None;
            }
            let datos = unsafe { core::slice::from_raw_parts(SISTEMA as *const u8, 24) };
            let r = dashos::maquina::sistema(datos).map(|(ram, _)| (ram, dashos::maquina::memoria_pedida(datos)));
            dashos::memoria_quitar(SISTEMA);
            r
        })
        .unwrap_or((0, 0));
    let programas = u32_en(p) as usize;
    p += 4;
    let cabeceras = p;
    // Tras las cabeceras, los nombres de los canales (16 bytes cada uno, en el orden de sus bits): para la tabla de
    // llaves de cada programa.
    let n_canales = (u32_en(cabeceras + CABECERA * programas) as usize).min(32);
    let nombres_canales = cabeceras + CABECERA * programas + 4;
    let nombre_canal = |c: usize| -> [u8; 16] {
        if c < n_canales { unsafe { core::ptr::read_volatile((nombres_canales + 16 * c) as *const [u8; 16]) } } else { [0; 16] }
    };
    let mut datos = nombres_canales + 16 * n_canales;
    // Los canales entre servicios: el extremo que reciben, en copias, los demás programas que lo nombran (0: aún no
    // creado). El primero recibe el otro extremo.
    let mut pendientes = [0u32; 32];
    // Quién lee cada canal (su cabecera), para saber cuál es el del servicio de archivos.
    let mut lector = [0usize; 32];
    // E6: los servicios (los que leen algún canal), para relanzarlos si acaban. Sus avisos de fin (K7) llegan por
    // `avisos`, con la marca del número del programa más 1.
    // Uno por programa: con más de 32 (desde la red, S13), los últimos se quedaban sin vigilar. Las marcas de los
    // clientes (`clientes_de`) llegan a 64.
    let mut servicios: [Option<Servicio>; 64] = core::array::from_fn(|_| None);
    // Los clientes de cada canal (bit = número del programa), para darle otra vez a su servicio, si se relanza, sus
    // entradas del directorio y, al de archivos, sus altas.
    let mut clientes_de = [0u64; 32];
    // El canal (sin marca) de `sesiones`, en cuanto se lance (U2), y lo lanzado antes, para darlo de alta entonces.
    let mut sesiones = 0u32;
    let mut antes = [(0usize, 0u64, 0u32, 0u32); 32];
    let mut n_antes = 0;
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
        // Qué es cada una (la tabla de llaves del programa): el nombre del aparato o el del canal.
        let mut nombres = [[0u8; 16]; 8];
        let mut k = 0;
        for j in 0..8 {
            let a = unsafe { *((cab + 8 + j) as *const u8) } as usize;
            if a < n && k < suyas.len() {
                nombres[k] = unsafe { core::ptr::read_volatile((aparatos + 24 * a) as *const [u8; 16]) };
                // Con UNA_VEZ: llega sin el derecho de transferirla (S4). Salvo a `sesiones`, que reparte los permisos
                // (U4, la llave de plazo): a él, con el derecho, para que pueda dar copias.
                let una_vez = if nombre(nombre_cab) == "sesiones" { 0 } else { UNA_VEZ };
                let copia = llave_duplicar(u32_en(aparatos + 24 * a + 20), 7 | una_vez, 0);
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
                    // U6: si lo sirve uno que sigue las sesiones y `sesiones` ya está, su copia sin marca (si no, se la
                    // da al lanzarlo).
                    if sesiones != 0 && va_a_sesiones(nombre(nombre_cab)) {
                        canal_a_sesiones(sesiones, b, nombre_cab);
                    }
                    lee |= 1 << c;
                    suyas[k] = a;
                    nombres[k] = nombre_canal(c);
                    k += 1;
                    if np < propios.len() {
                        propios[np] = b;
                        np += 1;
                    }
                } else {
                    // Cada cliente, con su marca (el número del programa más 1), y el servidor sabe de quién es por un
                    // mensaje sin marca que solo puede mandar `inicio` (auditoría de la fase 4, M6).
                    let marca = i as u64 + 1;
                    let reparte = nombre(nombre_cab) == "sesiones" && REPARTE.contains(&nombre(lector[c] + 16));
                    let copia = llave_duplicar(pendientes[c], 7, if reparte { 0 } else { marca });
                    if reparte {
                        // Sin entrada en el directorio: quien escribe por ella es `sesiones` en nombre de otros.
                        if copia > 0 {
                            suyas[k] = copia as u32;
                            nombres[k] = nombre_canal(c);
                            k += 1;
                        }
                        continue;
                    }
                    if copia > 0 {
                        suyas[k] = copia as u32;
                        nombres[k] = nombre_canal(c);
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
        for (j, &b) in propios[..np].iter().enumerate() {
            if k < suyas.len() {
                // A `sesiones`, sin marca (U2): con ella hace las copias marcadas que da a lo que lanza.
                let marca_propia = if nombre(nombre_cab) == "sesiones" { 0 } else { u64::MAX };
                let copia = llave_duplicar(b, dashos::ESCRIBIR | dashos::TRANSFERIR, marca_propia);
                if copia > 0 {
                    suyas[k] = copia as u32;
                    // "yo:" y el canal: el j-ésimo de los que lee.
                    let canal = (0..32).filter(|&c| lee & (1 << c) != 0).nth(j).map_or([0; 16], nombre_canal);
                    nombres[k][..3].copy_from_slice(b"yo:");
                    nombres[k][3..].copy_from_slice(&canal[..13]);
                    k += 1;
                }
            }
        }
        // A `sesiones`, además, una copia sin marca del canal de archivos (U2): con ella da de alta lo que lanza (con
        // sus raíces) y le hace su llave marcada con su número. La suya, marcada, va donde siempre; esta es la última (la 5).
        if nombre(nombre_cab) == "sesiones" && k < suyas.len() {
            if let Some(c) = (0..32).find(|&c| pendientes[c] != 0 && de_canales & (1 << c) != 0 && nombre(lector[c] + 16) == "archivos") {
                let copia = llave_duplicar(pendientes[c], 7, 0);
                if copia > 0 {
                    suyas[k] = copia as u32;
                    nombres[k] = dashos::llaves::nombre16("archivos.altas");
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
        // Topes de memoria de cada servicio (S3: desde los cupos como topes no se aparta nada al crear el grupo; lo que
        // no se pasa es lo usado de verdad). 16 MB por servicio; el de archivos, 64, para su caché de bloques (paso F9 de
        // docs/MULTITAREA_LOOP.md: con 6 MB de caché, un fichero de 8 MB leído de corrido iba siempre al disco).
        // U6: `sesiones`, 512: lo usado por los grupos de lo que lanza (64 MB cada uno) cuenta en el suyo.
        // C3: el fondo, 48: la imagen a pantalla entera en el montón y dos juegos de franjas (el que se ve y el nuevo); a
        // 1920 × 1080, unos 8 MB cada cosa.
        let cupo = match nombre(nombre_cab) {
            "archivos" => 64 << 20,
            "sesiones" => 512 << 20,
            "fondo" => 48 << 20,
            // El explorador: sus ventanas (3 MB cada una a 1100 × 720, 8 maximizada en Full HD), las fuentes, un PNG a
            // medio decodificar (hasta 1,5 Mpx) y 64 miniaturas. El Workbench, desde que los cajones los abre el
            // explorador, solo pinta los iconos y la Información: 16, lo de siempre.
            "explorador" => 32 << 20,
            // La pantalla de bloqueo y de entrada de `guardian` es de pantalla entera (8,3 MB a 1920 × 1080), con su
            // montón y la capa de las preguntas al lado.
            "guardian" => 32 << 20,
            // La caja Linux (H9): la RAM de su invitado de Linux sale de su cupo, más 66 MB del servicio (tablas de la etapa
            // 2, su montón con el disco del invitado y la terminal). Se calcula con la RAM de la máquina (decisión del
            // usuario del 2026-11-12 y del 2026-11-13): 256 MB con 2 GB, 512 con 4 y 1 GB con 8 o más. Sin Linux que
            // arrancar (sin su Image), solo el margen. Si no hay tanta memoria libre, la caja usa para el invitado lo que
            // le den (trozos de 16 MB hasta `E_MEMORIA`).
            "caja-linux" if tiene_aparato(cab, aparatos, n, "linux") => dashos::maquina::memoria_invitado_con(ram, pedida) + dashos::maquina::MARGEN_SERVICIO,
            "caja-linux" => dashos::maquina::MARGEN_SERVICIO,
            // H4 y H5: la caja Amiga, con su ejecutable (2 MB y 2 de datos), 4 MB de código traducido, 4 de montón de
            // Emu68, 8 del de Rust (las bibliotecas nativas y los lienzos: el del Workbench, de hasta 1024 x 768, son 3
            // MB), 4 del 68k, 1 de pilas y los búferes de sus ventanas: 32 MB. Y lo que crezca el 68k según sus
            // programas: hasta el ajuste `memoria amiga` de `Prefs/cajas` (256 MB de serie). El tope lo pone el kernel,
            // no la caja (un programa 68k podría engañarla). `inicio` no lee Prefs: la caja nace con el de serie, y
            // `sesiones`, que tiene la llave de su grupo (con el alta), le pone el del ajuste (llamada 54) al darla de
            // alta y cada vez que el centro lo cambia. S3: ya no hace falta la cuarta parte de lo libre.
            "caja-amiga" => (32 << 20) + MEMORIA_AMIGA_DE_SERIE,
            // aviso-cajas: la prueba del aviso de memoria baja llena la memoria de verdad (solo en las imágenes de prueba).
            "presion" => 1 << 40,
            _ => 16 << 20,
        };
        // De un servicio, inicio guarda una copia de cada llave, con los mismos derechos, para relanzarlo (E6).
        let mut guardadas = [0u32; 8];
        if lee != 0 {
            for j in 0..k {
                guardadas[j] = llave_duplicar(suyas[j], u64::MAX, 0).max(0) as u32;
            }
        }
        let g = grupo_crear(cupo);
        if g > 0 && nombre(nombre_cab) == "caja-linux" && cupo > dashos::maquina::MARGEN_SERVICIO {
            println!("  [inicio] caja-linux: {} MB de RAM en la máquina; el invitado, hasta {} MB", ram >> 20, dashos::maquina::memoria_invitado_con(ram, pedida) >> 20);
        }
        if g > 0 && nombre(nombre_cab) == "caja-amiga" {
            println!("  [inicio] caja-amiga: tope de {} MB, {} para que crezca su 68k", cupo >> 20, (cupo >> 20).saturating_sub(32));
        }
        let proceso = if g > 0 { proceso_crear_con_nombres(g as u32, m, tamano, &suyas[..k], &nombres[..k]) } else { g };
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
            for c in (0..32).filter(|&c| pendientes[c] != 0 && va_a_sesiones(nombre(lector[c] + 16))) {
                canal_a_sesiones(sesiones, pendientes[c], lector[c] + 16);
            }
            apagar_a_sesiones(sesiones);
            for &(cab, marca, grupo, p) in &antes[..n_antes] {
                alta_en_sesiones(sesiones, cab, marca, grupo);
                baja_en_sesiones(sesiones, p, marca);
            }
        } else if sesiones == 0 && n_antes < antes.len() {
            antes[n_antes] = (nombre_cab, i as u64 + 1, g as u32, proceso as u32);
            n_antes += 1;
        } else if sesiones != 0 {
            alta_en_sesiones(sesiones, nombre_cab, i as u64 + 1, g as u32);
            baja_en_sesiones(sesiones, proceso as u32, i as u64 + 1);
        }
        let mut vigilado = false;
        if lee != 0 && i < servicios.len() {
            let aviso = llave_duplicar(avisos_a, dashos::ESCRIBIR, i as u64 + 1).max(0) as u32;
            if aviso > 0 && dashos::proceso_avisar(proceso as u32, aviso) == 0 {
                servicios[i] = Some(Servicio { cab: nombre_cab, m, tamano, guardadas, nombres, k, cupo, lee, aviso, proceso: proceso as u32, grupo: g as u32, veces: 0 });
                vigilado = true;
            }
        }
        // La copia del ELF solo hace falta para relanzar un servicio: la de los demás se suelta (ya está cargado).
        dashos::memoria_quitar(destino);
        if !vigilado {
            dashos::cerrar(m);
        }
        // Los servicios de verdad (consola, USB) no acaban; los de prueba, sí: a esos se les espera. La caja Amiga
        // lleva el permiso de página 0 y también se espera (H4).
        if k == 0 || SE_ESPERAN.contains(&nombre(nombre_cab)) {
            let (clase, codigo) = proceso_esperar(proceso as u32);
            println!("  [inicio] {} acabó: clase {}, código {}", nombre(nombre_cab), clase, codigo);
        }
    }
    // Lo lanzado ya no necesita los ELF del paquete (los servicios tienen su copia para relanzarlos): solo sus cabeceras
    // (nombres, canales y aparatos, a los que apuntan `Servicio::cab` y `lector`). Se copian a un objeto pequeño que va
    // a la misma dirección, y el paquete entero (los ELF de todos, varios MB) se suelta.
    let largo = cabeceras + CABECERA * programas - PAQUETE;
    let pequeno = memoria_crear(largo);
    if (pequeno as i32) > 0 && memoria_proyectar(pequeno, COPIA_CABECERAS, true) == 0 {
        unsafe { core::ptr::copy_nonoverlapping(PAQUETE as *const u8, COPIA_CABECERAS as *mut u8, largo) };
        dashos::memoria_quitar(COPIA_CABECERAS);
        dashos::memoria_quitar(PAQUETE);
        if memoria_proyectar(pequeno, PAQUETE, false) == 0 {
            dashos::cerrar(1);
            println!("  [inicio] paquete de arranque suelto: quedan sus cabeceras ({} bytes)", largo);
        } else {
            // No debería pasar (la dirección acaba de quedar libre): el paquete otra vez, como estaba.
            let _ = memoria_proyectar(1, PAQUETE, false);
        }
    }
    println!("  [inicio] todo lanzado; se queda vigilando los servicios (a los {} ms del encendido)", dashos::ahora_us() / 1000);
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
        if !relanzar(s, &pendientes, &clientes_de, &lector, cabeceras, i as u64 + 1, sesiones) {
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
    /// Qué es cada una (la tabla de llaves).
    nombres: [[u8; 16]; 8],
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
fn relanzar(s: &mut Servicio, pendientes: &[u32; 32], clientes_de: &[u64; 32], lector: &[usize; 32], cabeceras: usize, marca: u64, sesiones: u32) -> bool {
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
    let proceso = if g > 0 { proceso_crear_con_nombres(g as u32, s.m, s.tamano, &suyas[..s.k], &s.nombres[..s.k]) } else { g };
    if proceso <= 0 {
        return false;
    }
    s.proceso = proceso as u32;
    s.grupo = g as u32;
    // En `sesiones`, otra vez su ficha con el grupo nuevo y su baja cuando acabe (U2).
    if sesiones != 0 {
        alta_en_sesiones(sesiones, s.cab, marca, s.grupo);
        baja_en_sesiones(sesiones, s.proceso, marca);
        // Si el relanzado es `sesiones`, otra vez su permiso de apagar.
        if nombre(s.cab) == "sesiones" {
            apagar_a_sesiones(sesiones);
        }
    }
    dashos::proceso_avisar(s.proceso, s.aviso) == 0
}

/// El ajuste `memoria amiga` de serie de `Prefs/cajas` (de 8 a 2048 MB).
const MEMORIA_AMIGA_DE_SERIE: u64 = 256 << 20;
