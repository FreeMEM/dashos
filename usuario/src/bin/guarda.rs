//! La prueba del selector de ficheros desde un programa del disco (`Programas/guarda`; lo lanza la barra, como la
//! persona que está delante): nace con la llave del selector, la encuentra por su nombre en su tabla de llaves
//! (`dashos::llaves`), guarda un fichero con "Guardar como" y lo sustituye después. Y lo hostil: lo que no debe
//! conseguir del selector un programa cualquiera.
//!
//! No ve nada de la ventana del selector: elige el usuario (en la prueba, el guion). Solo recibe la asa del fichero y
//! su nombre.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::selector as s;

dashos::programa!(principal);

/// Dónde ve su ventana del servicio de archivos.
const VA: usize = 0x50_0000_0000;

/// Sus llaves, por su nombre.
struct Llaves {
    archivos: u32,
    selector: u32,
    /// Por donde contesta archivos.
    lectura: u32,
}

impl Llaves {
    /// Una orden al servicio de archivos: (estado, asa, a).
    fn orden(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &str) -> (i32, u32, u64) {
        let mut p = [0u8; 160];
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        p[32..32 + nombre.len()].copy_from_slice(nombre.as_bytes());
        if dashos::canal_escribir(self.archivos, &p[..32 + nombre.len()], &[]) != 0 {
            return (-1, 0, 0);
        }
        let mut r = [0u8; 32];
        if dashos::canal_leer(self.lectura, &mut r, &mut []).0 != 32 {
            return (-1, 0, 0);
        }
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64::from_le_bytes(r[8..16].try_into().unwrap()))
    }

    /// Manda una petición al selector, sin esperar: el canal por el que llegará la respuesta, o 0.
    fn mandar(&self, orden: u32, propuesto: &str, patron: &str) -> u32 {
        let mut b = [0u8; 256];
        let Some(n) = s::escribir_peticion(&mut b, orden, propuesto, patron) else { return 0 };
        let (lectura, suyo) = dashos::canal_crear();
        if dashos::canal_escribir(self.selector, &b[..n], &[suyo]) != 0 {
            dashos::cerrar(lectura);
            dashos::cerrar(suyo);
            return 0;
        }
        lectura
    }

    /// Retira lo que tenga pedido.
    fn cancelar(&self) {
        let mut b = [0u8; 16];
        if let Some(n) = s::escribir_peticion(&mut b, s::CANCELAR, "", "") {
            dashos::canal_escribir(self.selector, &b[..n], &[]);
        }
    }
}

/// Lo que contesta el selector por `lectura`, esperando hasta `us`: (estado, asa, banderas, largo del nombre), con el
/// nombre en `nombre`. `i32::MIN` si no contesta o contesta algo que no vale.
fn esperar(lectura: u32, us: u64, nombre: &mut [u8; 128]) -> (i32, u32, u32, usize) {
    if lectura == 0 {
        return (i32::MIN, 0, 0, 0);
    }
    let mut r = [0u8; 256];
    let n = dashos::canal_leer_plazo(lectura, &mut r, &mut [], us).0;
    dashos::cerrar(lectura);
    match usize::try_from(n).ok().and_then(|n| s::leer_respuesta(&r[..n])) {
        Some(x) => {
            let l = x.nombre.len().min(nombre.len());
            nombre[..l].copy_from_slice(&x.nombre.as_bytes()[..l]);
            (x.estado, x.asa, x.banderas, l)
        }
        None => (i32::MIN, 0, 0, 0),
    }
}

fn texto(b: &[u8]) -> &str {
    core::str::from_utf8(b).unwrap_or("?")
}

fn principal() -> u64 {
    // Qué es cada llave lo dice quien lo lanzó: no se cuenta cuántas trae.
    let t = dashos::llaves::Tabla::leer();
    let (Some(archivos), Some(selector)) = (t.buscar("archivos"), t.buscar("selector")) else {
        println!("  [guarda] sin la llave de archivos o la del selector en su tabla ({} llaves): MAL", t.cuantas());
        return 1;
    };
    println!("  [guarda] tabla de llaves: sesiones {}, archivos {}, ventanas {}, selector {}",
        t.buscar("sesiones").unwrap_or(0), archivos, t.buscar("ventanas").unwrap_or(0), selector);
    let (lectura, suyo) = dashos::canal_crear();
    let ll = Llaves { archivos, selector, lectura };
    let mut sesion = [0u8; a::CABECERA];
    sesion[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut ventana = [0u32; 1];
    let mut r = [0u8; 32];
    if dashos::canal_escribir(archivos, &sesion, &[suyo]) != 0 || dashos::canal_leer(lectura, &mut r, &mut ventana).0 != 32 || dashos::memoria_proyectar(ventana[0], VA, true) != 0 {
        println!("  [guarda] sin sesión en archivos: MAL");
        return 1;
    }
    let datos = unsafe { core::slice::from_raw_parts_mut(VA as *mut u8, 4096) };
    let mut nombre = [0u8; 128];

    // Lo que no pide ventana. Quién dirá el selector que es; una petición mal formada; y `C` entero como raíz, que solo
    // es para la prueba del hostil de `inicio`.
    let (consulta, _, _, l) = esperar(ll.mandar(s::CONSULTA, "", ""), 5_000_000, &mut nombre);
    let quien_es = consulta == s::BIEN && &nombre[..l] == b"guarda";
    let mala = {
        let (lectura, suyo) = dashos::canal_crear();
        let mut b = [0u8; 20];
        b[..4].copy_from_slice(&s::MAGIA);
        b[4..8].copy_from_slice(&77u32.to_le_bytes());
        if dashos::canal_escribir(selector, &b, &[suyo]) == 0 { esperar(lectura, 5_000_000, &mut nombre).0 } else { i32::MIN }
    };
    let raiz = {
        let (lectura, suyo) = dashos::canal_crear();
        let mut r = [0xffu8; 4];
        let n = if dashos::canal_escribir(selector, b"raiz", &[suyo]) == 0 { dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0 } else { -1 };
        dashos::cerrar(lectura);
        if n == 4 { u32::from_le_bytes(r) } else { u32::MAX }
    };
    // Por su raíz 1 (el disco, solo para leer) no crea nada en `C`.
    let (e_c, c, _) = ll.orden(a::BUSCAR, 1, 0, 0, "C");
    let crear = if e_c == a::BIEN { ll.orden(a::CREAR, c, a::FICHERO as u64, 0, "colado.txt").0 } else { e_c };
    if e_c == a::BIEN {
        ll.orden(a::CERRAR, c, 0, 0, "");
    }
    let bien0 = quien_es && mala == s::MAL_FORMADO && raiz == 0 && crear == a::PROTEGIDO;
    println!("  [guarda] sin ventana: el selector lo conoce como \"{}\" ({}), una petición mal formada {}, C entero como raíz {}, crear en C por su cuenta {}: {}",
        texto(&nombre[..l]), consulta, mala, raiz, crear, if bien0 { "bien" } else { "MAL" });

    // Dos peticiones a la vez: la segunda, ocupado. Y cancelar a medias: la primera se retira (con la ventana en
    // pantalla o antes de que salga) y contesta cancelado. La primera propone un camino en vez de un nombre.
    let primera = ll.mandar(s::GUARDAR, "../../Estado/Servicios/sesiones/registro", "");
    let (segunda, ..) = esperar(ll.mandar(s::GUARDAR, "otro.txt", ""), 5_000_000, &mut nombre);
    dashos::dormir_us(1_500_000);
    ll.cancelar();
    let (cancelada, asa, ..) = esperar(primera, 10_000_000, &mut nombre);
    // Más nombres propuestos que no valen, retirados en el acto: con '/', con ':', "..", y larguísimo.
    let largo = [b'x'; 200];
    let mut retiradas = 0;
    for propuesto in ["a/b", "Home:secreto", "..", texto(&largo)] {
        let pedida = ll.mandar(s::GUARDAR, propuesto, "");
        ll.cancelar();
        if esperar(pedida, 10_000_000, &mut nombre).0 == s::CANCELADO {
            retiradas += 1;
        }
    }
    let bien1 = segunda == s::OCUPADO && cancelada == s::CANCELADO && asa == 0 && retiradas == 4;
    println!("  [guarda] dos a la vez: la segunda {}; cancelar a medias: {} (asa {}); con nombres que no valen, retiradas {} de 4: {}",
        segunda, cancelada, asa, retiradas, if bien1 { "bien" } else { "MAL" });

    // "Guardar como": propone informe.txt y solo quiere ver textos. Elige el usuario; aquí llegan la asa y el nombre.
    let guardar = |ll: &Llaves, nombre: &mut [u8; 128], contenido: &[u8], datos: &mut [u8]| -> (i32, u32, usize, bool) {
        let (estado, asa, banderas, l) = esperar(ll.mandar(s::GUARDAR, "informe.txt", "*.txt"), 180_000_000, nombre);
        if estado != s::BIEN {
            return (estado, banderas, l, false);
        }
        // Lo escribe entero y lo deja de ese tamaño (si existía, el selector lo da como estaba), y lo lee.
        datos[..contenido.len()].copy_from_slice(contenido);
        let escribir = ll.orden(a::ESCRIBIR, asa, 0, contenido.len() as u64, "").0;
        let tamano = ll.orden(a::TAMANO, asa, contenido.len() as u64, 0, "").0;
        datos[..64].fill(0);
        let (leer, _, leidos) = ll.orden(a::LEER, asa, 0, 64, "");
        let igual = leer == a::BIEN && leidos as usize == contenido.len() && datos[..contenido.len()] == *contenido;
        // Con la asa de un fichero no se llega a su cajón ni a nada más: ni buscar desde ella ni listar.
        let buscar = ll.orden(a::BUSCAR, asa, 0, 0, "Privado").0;
        let listar = ll.orden(a::LISTAR, asa, 0, 0, "").0;
        ll.orden(a::CERRAR, asa, 0, 0, "");
        (estado, banderas, l, escribir == a::BIEN && tamano == a::BIEN && igual && buscar != a::BIEN && listar != a::BIEN)
    };
    let (e1, b1, l1, bien_1) = guardar(&ll, &mut nombre, "primera versión, bastante más larga\n".as_bytes(), datos);
    let nuevo = e1 == s::BIEN && b1 == 0 && bien_1 && &nombre[..l1] == b"informe.txt";
    println!("  [guarda] guardar: {}, \"{}\" (solo el nombre), {}; lo escribe y lo lee por la asa, y con ella no llega a más: {}",
        e1, texto(&nombre[..l1]), if b1 & s::EXISTIA != 0 { "ya existía" } else { "nuevo" }, if nuevo { "bien" } else { "MAL" });
    let (e2, b2, l2, bien_2) = guardar(&ll, &mut nombre, "segunda versión\n".as_bytes(), datos);
    let sustituido = e2 == s::BIEN && b2 == s::EXISTIA && bien_2 && &nombre[..l2] == b"informe.txt";
    println!("  [guarda] guardar otra vez: {}, \"{}\", {}; queda del tamaño de lo nuevo: {}",
        e2, texto(&nombre[..l2]), if b2 & s::EXISTIA != 0 { "ya existía y el usuario dijo sustituir" } else { "nuevo" }, if sustituido { "bien" } else { "MAL" });
    // "Abrir", enseñando solo lo que casa con su patrón: el usuario elige ese mismo fichero, y llega solo para leer. Es
    // suyo (del mismo usuario), pero con la asa cedida para leer no lo cambia: ni escribe, ni lo recorta, ni le pone
    // fecha.
    let (e3, asa, _, l3) = esperar(ll.mandar(s::ABRIR, "", "informe*"), 180_000_000, &mut nombre);
    datos[..64].fill(0);
    let (leer, _, leidos) = ll.orden(a::LEER, asa, 0, 64, "");
    let leido = leer == a::BIEN && &datos[..leidos as usize] == "segunda versión\n".as_bytes();
    datos[..4].copy_from_slice(b"otra");
    let escribir = ll.orden(a::ESCRIBIR, asa, 0, 4, "").0;
    let recortar = ll.orden(a::TAMANO, asa, 0, 0, "").0;
    let fechar = ll.orden(a::FECHAR, asa, 1, 0, "").0;
    let (tamano, _, mide) = ll.orden(a::DATOS, asa, 0, 0, "");
    ll.orden(a::CERRAR, asa, 0, 0, "");
    let solo_leer = e3 == s::BIEN && &nombre[..l3] == b"informe.txt" && leido && escribir == a::PROTEGIDO && recortar == a::PROTEGIDO && fechar == a::PROTEGIDO;
    println!("  [guarda] abrir: {}, \"{}\"; lo lee {}; escribirlo {}, recortarlo {}, ponerle fecha {} (datos {}, mide {}): {}",
        e3, texto(&nombre[..l3]), leer, escribir, recortar, fechar, tamano, mide, if solo_leer { "bien" } else { "MAL" });
    let bien = bien0 && bien1 && nuevo && sustituido && solo_leer;
    println!("  [guarda] acabado: {}", if bien { "bien" } else { "MAL" });
    !bien as u64
}
