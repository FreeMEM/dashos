//! U4: un programa con permiso para `eco`. Pide su llave a `sesiones`, la usa, renuncia y comprueba que la llave ya
//! no sirve (revocada). Llave 1: el canal "sesiones". U2: su tabla de nombres y, lanzado desde el disco, sus raíces
//! en archivos (llave 2, la que le da `sesiones` al lanzarlo).

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::sesiones::{Nombres, DEJAR, NOMBRES, SERVICIO};

dashos::programa!(principal);

const SESIONES: u32 = 1;
const ARCHIVOS: u32 = 2;

/// Una petición a `sesiones` con el nombre de un servicio: (estado, llave que llegue).
fn pedir(orden: u32, servicio: &str) -> (i32, u32) {
    let mut p = [0u8; 64];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&(servicio.len() as u32).to_le_bytes());
    p[12..12 + servicio.len()].copy_from_slice(servicio.as_bytes());
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 4];
    let mut ll = [0u32; 1];
    let (n, k, _) = if dashos::canal_escribir(SESIONES, &p[..12 + servicio.len()], &[suyo]) == 0 {
        // Si hace falta, contesta el usuario (U4): hasta 30 s.
        let (n, k, m) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 35_000_000);
        (n, k, m)
    } else {
        (-1, 0, 0)
    };
    dashos::cerrar(lectura);
    (if n == 4 { i32::from_le_bytes(r) } else { i32::MIN }, if k == 1 { ll[0] } else { 0 })
}

/// Manda "hola" por `eco` y devuelve si contestó "eco: hola", o el error de escribir.
fn usar(eco: u32) -> Result<bool, i64> {
    let (lectura, suyo) = dashos::canal_crear();
    let e = dashos::canal_escribir(eco, b"hola", &[suyo]);
    if e != 0 {
        dashos::cerrar(lectura);
        dashos::cerrar(suyo);
        return Err(e);
    }
    let mut r = [0u8; 32];
    let (n, _, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut [], 2_000_000);
    dashos::cerrar(lectura);
    Ok(n > 0 && &r[..n as usize] == b"eco: hola")
}

fn principal() -> u64 {
    dashos::dormir_us(1_500_000);
    let (estado, eco) = pedir(SERVICIO, "eco");
    let contesta = eco != 0 && usar(eco) == Ok(true);
    // Sin el derecho de transferirla (S4): no puede regalarla.
    let (a, b) = dashos::canal_crear();
    let regalar = dashos::canal_escribir(a, b"toma", &[eco]);
    dashos::cerrar(a);
    dashos::cerrar(b);
    let (dejar, _) = pedir(DEJAR, "eco");
    let tras_dejar = usar(eco);
    let bien = estado == 0 && contesta && regalar == -3 && dejar == 0 && tras_dejar.is_err();
    println!(
        "  [pide] eco: con permiso {} y contesta {}; regalarla da {}; tras dejarla, usarla da {:?}: {}",
        estado,
        contesta,
        regalar,
        tras_dejar,
        if bien { "bien" } else { "MAL" }
    );
    if bien {
        println!("  [pide] U4: bien");
    }
    // U4, la llave de plazo: un permiso del kernel que también reparte `sesiones`. Con ella entra en la clase de plazo;
    // tras dejarla, ya no.
    let (e_plazo, plazo) = pedir(SERVICIO, "plazo");
    // Poco (un 1 %): lo prometido en un núcleo tiene tope, y los servicios de sonido pueden estar en el mismo.
    let entra = plazo != 0 && dashos::plazo(plazo, 200, 20_000) == 0;
    if entra {
        dashos::plazo(plazo, 0, 0);
    }
    let (dejar_plazo, _) = pedir(DEJAR, "plazo");
    let despues = plazo != 0 && dashos::plazo(plazo, 200, 20_000) == 0;
    let plazo_bien = e_plazo == 0 && entra && dejar_plazo == 0 && !despues;
    println!("  [pide] llave de plazo: pedirla {}, entrar en la clase {}, dejarla {}, después entra {}: {}", e_plazo, entra, dejar_plazo, despues,
        if plazo_bien { "bien" } else { "MAL" });
    let (nombres, progdir) = nombres();
    if nombres {
        println!("  [pide] U2, tabla de nombres {}: bien", if progdir { "con PROGDIR" } else { "sin PROGDIR (no salió del disco)" });
    }
    // U3: lanzado como Carla (con su sesión abierta), al acabar pide bloquear su sesión; la pantalla la pone `guardian`.
    // Sin cuenta (el usuario 1, sin sesión), no puede: -2.
    let con_sesion = nombres_home();
    let bloquear = {
        let (lect, suyo) = dashos::canal_crear();
        let mut r = [0u8; 4];
        let n = if dashos::canal_escribir(SESIONES, &dashos::protocolo::sesiones::BLOQUEAR.to_le_bytes(), &[suyo]) == 0 {
            dashos::canal_leer_plazo(lect, &mut r, &mut [], 5_000_000).0
        } else {
            -1
        };
        dashos::cerrar(lect);
        let e = if n == 4 { i32::from_le_bytes(r) } else { i32::MIN };
        if con_sesion {
            println!("  [pide] bloquear la sesión de Carla: {}", e);
        } else {
            println!("  [pide] bloquear sin tener sesión: {}", e);
        }
        e == if con_sesion { 0 } else { -2 }
    };
    if bien && nombres && plazo_bien && bloquear { 0 } else { 1 }
}

/// Si su tabla de nombres trae `Home:` (lanzado como un usuario con cuenta).
fn nombres_home() -> bool {
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 256];
    let n = if dashos::canal_escribir(SESIONES, &NOMBRES.to_le_bytes(), &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    let t = Nombres::desde(&r[..n.max(0) as usize]);
    let mut sal = [0u8; 64];
    t.resolver("Home:", &mut sal).is_some()
}

/// U2: su tabla de nombres. Usuario 1 (sin cuenta): sin `Home:`. Lanzado del disco por `entrada`, con `PROGDIR:` en
/// el cajón de donde salió; lanzado por `inicio`, sin él. Lo que intenta salir del subcamino no se resuelve. Devuelve
/// si todo fue bien y si tenía `PROGDIR:`.
fn nombres() -> (bool, bool) {
    let (lectura, suyo) = dashos::canal_crear();
    let mut r = [0u8; 256];
    let n = if dashos::canal_escribir(SESIONES, &NOMBRES.to_le_bytes(), &[suyo]) == 0 {
        dashos::canal_leer_plazo(lectura, &mut r, &mut [], 5_000_000).0
    } else {
        -1
    };
    dashos::cerrar(lectura);
    if n < 4 {
        println!("  [pide] U2: sin tabla de nombres ({})", n);
        return (false, false);
    }
    let t = Nombres::desde(&r[..n as usize]);
    for i in 0..t.len() {
        if let Some((nombre, raiz, vol, sub)) = t.entrada(i) {
            fn s(x: &[u8]) -> &str {
                core::str::from_utf8(x).unwrap_or("?")
            }
            match raiz {
                Some(r) => println!("  [pide] {}: raíz {}, {}", s(nombre), r, s(sub)),
                None => println!("  [pide] {}: en {} {}", s(nombre), if vol.is_empty() { "el disco," } else { s(vol) }, s(sub)),
            }
        }
    }
    let mut sal = [0u8; 64];
    let mut va = |c: &str, vol: &str, ruta: Option<&str>| {
        let r = t.resolver(c, &mut sal);
        let bien = match (r, ruta) {
            (Some((_, v, x)), Some(e)) => v == vol.as_bytes() && x == e,
            (None, None) => true,
            _ => false,
        };
        if !bien {
            println!("  [pide] U2: {} no resuelve como debe", c);
        }
        bien
    };
    let mut probe = [0u8; 64];
    let progdir = t.resolver("PROGDIR:", &mut probe).is_some();
    // Lanzado del disco, `PROGDIR:` es su raíz 0 (el cajón `Programas`); sin él, no hay.
    let a = va("PROGDIR:pide", "", progdir.then_some("pide"));
    let b = va("progdir:otro/x", "", progdir.then_some("otro/x"));
    let c = va("SYS:", "", Some(""));
    let mut probe2 = [0u8; 64];
    let home = t.resolver("Home:", &mut probe2).is_some();
    let d = home || va("Home:notas", "", None);
    let e = va("PROGDIR:/hola", "", None);
    let f = va("SYS:a/../../b", "", None);
    let g = va("sin dos puntos", "", None);
    let raices = !progdir || raices(&t, home);
    (t.len() == 2 + progdir as usize + home as usize && a && b && c && d && e && f && g && raices, progdir)
}

/// El número del primer proceso con ese nombre en la lista de `sesiones`, o 0.
fn buscar_proceso(nombre: &[u8]) -> u64 {
    use dashos::protocolo::sesiones::{FICHA, POR_PAGINA, PROCESOS};
    let mut desde = 0u64;
    loop {
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&PROCESOS.to_le_bytes());
        p[4..12].copy_from_slice(&desde.to_le_bytes());
        let (lect, suyo) = dashos::canal_crear();
        let mut r = [0u8; 8 + FICHA * POR_PAGINA];
        let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lect, &mut r, &mut [], 3_000_000).0 } else { -1 };
        dashos::cerrar(lect);
        if n < 8 {
            return 0;
        }
        let van = u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize;
        for i in 0..van {
            let f = &r[8 + FICHA * i..8 + FICHA * (i + 1)];
            let nom = &f[16..32];
            if &nom[..nom.iter().position(|&c| c == 0).unwrap_or(16)] == nombre {
                return u64::from_le_bytes(f[0..8].try_into().unwrap());
            }
        }
        if van < POR_PAGINA {
            return 0;
        }
        desde += van as u64;
    }
}

/// Una orden al servicio de archivos: (estado, asa).
fn orden(lectura: u32, orden: u32, asa: u32, x: u64, nombre: &str) -> (i32, u32) {
    let mut p = [0u8; 96];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&asa.to_le_bytes());
    p[8..16].copy_from_slice(&x.to_le_bytes());
    p[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
    p[32..32 + nombre.len()].copy_from_slice(nombre.as_bytes());
    if dashos::canal_escribir(ARCHIVOS, &p[..32 + nombre.len()], &[]) != 0 {
        return (i32::MIN, 0);
    }
    let mut r = [0u8; 32];
    if dashos::canal_leer_plazo(lectura, &mut r, &mut [], 3_000_000).0 != 32 {
        return (i32::MIN, 0);
    }
    (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()))
}

/// U2: lanzado desde el disco nace con su llave de archivos y sus raíces, las de su tabla: la 0 su cajón (`PROGDIR:`)
/// y la 1 el disco para leer (`SYS:`). Abre su sesión y busca por la tabla; por `SYS:` no crea (tope de solo leer) y
/// no cierra sus raíces.
fn raices(t: &Nombres, home: bool) -> bool {
    use dashos::protocolo::archivos as a;
    let (lectura, suyo) = dashos::canal_crear();
    let mut s = [0u8; a::CABECERA];
    s[0..4].copy_from_slice(&a::SESION.to_le_bytes());
    let mut r = [0u8; 32];
    let mut ventana = [0u32; 1];
    let sesion = dashos::canal_escribir(ARCHIVOS, &s, &[suyo]) == 0
        && dashos::canal_leer_plazo(lectura, &mut r, &mut ventana, 3_000_000).0 == 32
        && i32::from_le_bytes(r[0..4].try_into().unwrap()) == a::BIEN;
    if !sesion {
        println!("  [pide] U2: sin sesión en archivos ({})", i32::from_le_bytes(r[0..4].try_into().unwrap()));
        dashos::cerrar(lectura);
        return false;
    }
    let mut buscar = |camino: &str| -> (i32, u32) {
        let mut sal = [0u8; 64];
        match t.resolver(camino, &mut sal) {
            Some((Some(raiz), _, ruta)) => orden(lectura, a::BUSCAR, raiz as u32, 0, ruta),
            _ => (i32::MIN, 0),
        }
    };
    let (yo, asa_yo) = buscar("PROGDIR:pide");
    let (hola, asa_hola) = buscar("Programas:hola");
    let (leeme, asa_leeme) = buscar("SYS:LEEME.txt");
    for (e, asa) in [(yo, asa_yo), (hola, asa_hola), (leeme, asa_leeme)] {
        if e == a::BIEN {
            orden(lectura, a::CERRAR, asa, 0, "");
        }
    }
    let crear = orden(lectura, a::CREAR, 1, a::FICHERO as u64, "mio.txt").0;
    let cerrar = orden(lectura, a::CERRAR, 1, 0, "").0;
    // Lanzado como Carla: en su `Home:` (su volumen, raíz 2) crea y borra.
    let en_home = !home || {
        let mut sal = [0u8; 64];
        match t.resolver("Home:", &mut sal) {
            Some((Some(raiz), _, "")) => {
                let (c, asa) = orden(lectura, a::CREAR, raiz as u32, a::FICHERO as u64, "de-pide.txt");
                if c == a::BIEN {
                    orden(lectura, a::CERRAR, asa, 0, "");
                }
                let b = orden(lectura, a::BORRAR, raiz as u32, 0, "de-pide.txt").0;
                println!("  [pide] Home: crea {} y borra {} en el volumen de Carla: {}", c, b, if c == a::BIEN && b == a::BIEN { "bien" } else { "MAL" });
                c == a::BIEN && b == a::BIEN
            }
            _ => false,
        }
    };
    dashos::cerrar(lectura);
    dashos::cerrar(ventana[0]);
    // U5: lanzado como Carla, pide una acción de administrador: comprobar el disco. `guardian` da la contraseña de Ana.
    let admin = !home || {
        use dashos::protocolo::sesiones::ADMINISTRAR;
        let mut p = [0u8; 21];
        p[0..4].copy_from_slice(&ADMINISTRAR.to_le_bytes());
        p[4..8].copy_from_slice(&9u32.to_le_bytes());
        p[12..21].copy_from_slice(b"comprobar");
        let (lect, suyo) = dashos::canal_crear();
        let mut r = [0u8; 12];
        let n = if dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lect, &mut r, &mut [], 10_000_000).0 } else { -1 };
        dashos::cerrar(lect);
        let (e, problemas) = (i32::from_le_bytes(r[0..4].try_into().unwrap()), u64::from_le_bytes(r[4..12].try_into().unwrap()));
        let bien = n == 12 && e == 0 && problemas == 0;
        println!("  [pide] acción de administrador (comprobar el disco): {} con {} problemas: {}", e, problemas, if bien { "bien" } else { "MAL" });
        // Y matar a `eco`, un servicio del sistema (no es suyo): también es acción de administrador. `inicio` lo
        // relanza (E6).
        let eco = buscar_proceso(b"eco");
        let mut p = [0u8; 25];
        p[0..4].copy_from_slice(&ADMINISTRAR.to_le_bytes());
        p[4..8].copy_from_slice(&5u32.to_le_bytes());
        p[12..17].copy_from_slice(b"matar");
        p[17..25].copy_from_slice(&eco.to_le_bytes());
        let (lect, suyo) = dashos::canal_crear();
        let n = if eco != 0 && dashos::canal_escribir(SESIONES, &p, &[suyo]) == 0 { dashos::canal_leer_plazo(lect, &mut r, &mut [], 10_000_000).0 } else { -1 };
        dashos::cerrar(lect);
        let matar = i32::from_le_bytes(r[0..4].try_into().unwrap());
        let sigue = buscar_proceso(b"eco") == eco;
        println!("  [pide] acción de administrador (matar a eco, proceso {}): {}, ya no está con ese número: {}", eco, matar, if n == 12 && matar == 0 && !sigue { "bien" } else { "MAL" });
        bien && n == 12 && matar == 0 && !sigue
    };
    let bien = yo == a::BIEN && hola == a::BIEN && leeme == a::BIEN && crear == a::PROTEGIDO && cerrar != a::BIEN && en_home && admin;
    println!("  [pide] raíces: PROGDIR:pide {}, Programas:hola {}, SYS:LEEME.txt {}; crear por SYS: {}, cerrarla {}: {}",
        yo, hola, leeme, crear, cerrar, if bien { "bien" } else { "MAL" });
    bien
}
