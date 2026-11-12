//! U6, cambio rápido de usuario (`docs/diseño/U6-cambio-rapido.md`): el programa de la prueba en `virt`. Está en el
//! disco (`Programas/cambio`) y `guardian` lo lanza dos veces; hace un papel u otro según de quién sea su `Home:`.
//!
//! - **Ana (el testigo)**: escribe `Home:testigo.txt` y espera a que entre otra persona. Con su sesión detrás sigue en
//!   marcha: escribe más en su fichero, y pedir la llave de un servicio o bloquear se le niega sin preguntar (quien
//!   está delante es otra persona). Cuando Ana vuelve, su fichero tiene todo lo escrito.
//! - **Carla (la espía)**: con la sesión de Ana detrás, intenta ver y matar sus procesos, entrar, volver, salir o lanzar
//!   como ella, abrir, ver o cerrar su volumen (con la contraseña buena) y llegar a su fichero por sus raíces. Todo
//!   tiene que fallar.
//!
//! Llave 1: su canal con `sesiones`; llave 2: su llave de archivos (las dos, marcadas con su número).

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::archivos as a;
use dashos::protocolo::sesiones as s;

dashos::programa!(principal);

const SESIONES: u32 = 1;
const ARCHIVOS: u32 = 2;
/// Dónde ve su ventana del servicio de archivos.
const VA_VENTANA: usize = 0x40_0000_0000;
/// Lo que escribe el testigo, delante y detrás: 48 letras que se calculan al correr (`a` + (7i + semilla) mod 26), para
/// que no estén en claro en el programa, que está en el disco. `tools/disco_archivos.py` las busca en el disco: con el
/// volumen cifrado, no tienen que salir.
const LARGO: usize = 48;
fn texto(semilla: usize) -> [u8; LARGO] {
    core::array::from_fn(|i| b'a' + ((7 * i + semilla) % 26) as u8)
}

/// Una petición a `sesiones` con una llave de respuesta: (bytes leídos, llaves que llegan) en `r`.
fn pedir(p: &[u8], r: &mut [u8], plazo_us: u64) -> (i64, usize) {
    let (lectura, suyo) = dashos::canal_crear();
    let mut ll = [0u32; 1];
    let (n, k) = if dashos::canal_escribir(SESIONES, p, &[suyo]) == 0 {
        let (n, k, _) = dashos::canal_leer_plazo(lectura, r, &mut ll, plazo_us);
        (n, k)
    } else {
        (-1, 0)
    };
    dashos::cerrar(lectura);
    if k == 1 {
        dashos::cerrar(ll[0]);
    }
    (n, k)
}

/// Una orden de `sesiones` que contesta un i32.
fn estado(p: &[u8]) -> i32 {
    let mut r = [0u8; 16];
    match pedir(p, &mut r, 5_000_000) {
        (n, _) if n >= 4 => i32::from_le_bytes(r[0..4].try_into().unwrap()),
        _ => i32::MIN,
    }
}

/// Orden, dos largos y dos textos (entrar, volver, salir y pedir un servicio).
fn con_textos(orden: u32, x: &[u8], y: &[u8]) -> i32 {
    let mut p = [0u8; 96];
    p[0..4].copy_from_slice(&orden.to_le_bytes());
    p[4..8].copy_from_slice(&(x.len() as u32).to_le_bytes());
    p[8..12].copy_from_slice(&(y.len() as u32).to_le_bytes());
    p[12..12 + x.len()].copy_from_slice(x);
    p[12 + x.len()..12 + x.len() + y.len()].copy_from_slice(y);
    let r = estado(&p[..12 + x.len() + y.len()]);
    p.fill(0);
    r
}

fn activa() -> i32 {
    estado(&s::ACTIVA.to_le_bytes())
}

/// Su sesión con el servicio de archivos.
struct Archivos {
    lectura: u32,
}

impl Archivos {
    fn abrir() -> Option<Archivos> {
        let (lectura, suyo) = dashos::canal_crear();
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        if dashos::canal_escribir(ARCHIVOS, &p, &[suyo]) != 0 {
            return None;
        }
        let mut r = [0u8; 32];
        let mut ll = [0u32; 1];
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut r, &mut ll, 3_000_000);
        if n != 32 || k != 1 || i32::from_le_bytes(r[0..4].try_into().unwrap()) != a::BIEN || dashos::memoria_proyectar(ll[0], VA_VENTANA, true) != 0 {
            return None;
        }
        Some(Archivos { lectura })
    }

    /// Una orden: (estado, asa, a) de la respuesta.
    fn orden(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> (i32, u32, u64) {
        let mut p = [0u8; 128];
        let largo = nombre.len().min(96);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(largo as u32).to_le_bytes());
        p[32..32 + largo].copy_from_slice(&nombre[..largo]);
        if dashos::canal_escribir(ARCHIVOS, &p[..32 + largo], &[]) != 0 {
            return (i32::MIN, 0, 0);
        }
        let mut r = [0u8; 32];
        if dashos::canal_leer_plazo(self.lectura, &mut r, &mut [], 3_000_000).0 != 32 {
            return (i32::MIN, 0, 0);
        }
        (i32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()), u64::from_le_bytes(r[8..16].try_into().unwrap()))
    }

    fn ventana(&self) -> &'static mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(VA_VENTANA as *mut u8, 4096) }
    }

    fn escribir(&self, asa: u32, desde: u64, datos: &[u8]) -> i32 {
        self.ventana()[..datos.len()].copy_from_slice(datos);
        self.orden(a::ESCRIBIR, asa, desde, datos.len() as u64, b"").0
    }
}

/// La raíz de `Home:` y el nombre de su volumen, por su tabla de nombres.
fn home() -> Option<(u8, [u8; 16])> {
    let mut r = [0u8; 256];
    let (n, _) = pedir(&s::NOMBRES.to_le_bytes(), &mut r, 5_000_000);
    let t = s::Nombres::desde(&r[..n.max(0) as usize]);
    let mut sal = [0u8; 64];
    let (raiz, vol, _) = t.resolver("Home:", &mut sal)?;
    let mut v = [0u8; 16];
    v[..vol.len().min(16)].copy_from_slice(&vol[..vol.len().min(16)]);
    Some((raiz?, v))
}

/// Espera hasta `max_us` a que `ACTIVA` dé `quiere`.
fn esperar_activa(quiere: i32, max_us: u64) -> bool {
    let hasta = dashos::ahora_us() + max_us;
    while dashos::ahora_us() < hasta {
        if activa() == quiere {
            return true;
        }
        dashos::dormir_us(300_000);
    }
    false
}

fn principal() -> u64 {
    let Some((raiz, vol)) = home() else {
        println!("  [cambio] sin Home: en su tabla de nombres: MAL");
        return 1;
    };
    let Some(arch) = Archivos::abrir() else {
        println!("  [cambio] sin sesión en archivos: MAL");
        return 1;
    };
    let bien = match &vol[..vol.iter().position(|&c| c == 0).unwrap_or(16)] {
        b"Ana" => testigo(&arch, raiz),
        b"Carla" => espia(&arch, raiz),
        _ => {
            println!("  [cambio] Home: de nadie conocido: MAL");
            false
        }
    };
    if bien { 0 } else { 1 }
}

/// Ana: su programa sigue detrás y lo encuentra todo al volver.
fn testigo(arch: &Archivos, home: u8) -> bool {
    let al_empezar = activa();
    let (e, asa, _) = arch.orden(a::CREAR, home as u32, a::FICHERO as u64, 0, b"testigo.txt");
    // Con `black_box`, el compilador no las calcula de antemano (saldrían en claro en el programa).
    let (delante, detras_) = (texto(core::hint::black_box(0)), texto(core::hint::black_box(3)));
    let escrito = e == a::BIEN && arch.escribir(asa, 0, &delante) == a::BIEN;
    println!("  [cambio] Ana: delante {}; escribe Home:testigo.txt {}; espera a que entre otra persona", al_empezar, escrito);
    if !escrito || !esperar_activa(0, 120_000_000) {
        println!("  [cambio] Ana: su sesión no pasó detrás: MAL");
        return false;
    }
    // Detrás: sigue en marcha y con su volumen; no pregunta ni bloquea.
    let escribe = arch.escribir(asa, LARGO as u64, &detras_) == a::BIEN;
    let eco = con_textos(s::SERVICIO, b"eco", b"");
    let bloquear = estado(&s::BLOQUEAR.to_le_bytes());
    let detras = escribe && eco == s::PROTEGIDO && bloquear == s::PROTEGIDO;
    println!("  [cambio] Ana, detrás: escribe {}, pedir eco {}, bloquear {}: {}", escribe, eco, bloquear, if detras { "bien" } else { "MAL" });
    if !esperar_activa(1, 120_000_000) {
        println!("  [cambio] Ana: no vuelve delante: MAL");
        return false;
    }
    let (e, _, n) = arch.orden(a::LEER, asa, 0, 2 * LARGO as u64, b"");
    let v = &arch.ventana()[..2 * LARGO];
    let sigue = e == a::BIEN && n as usize == 2 * LARGO && v[..LARGO] == delante && v[LARGO..] == detras_;
    arch.orden(a::CERRAR, asa, 0, 0, b"");
    let bien = detras && sigue;
    println!("  [cambio] Ana vuelve delante: su fichero sigue, con lo escrito detrás: {}", if bien { "bien" } else { "MAL" });
    bien
}

/// Carla: con Ana detrás, nada de lo de Ana está a su alcance.
fn espia(arch: &Archivos, home: u8) -> bool {
    let delante = activa();
    // Los procesos que ve: ninguno de otra persona.
    let mut vistos = [0u64; 64];
    let mut cuantos = 0;
    let mut ajenos = 0;
    loop {
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&s::PROCESOS.to_le_bytes());
        p[4..12].copy_from_slice(&(cuantos as u64).to_le_bytes());
        let mut r = [0u8; 8 + s::FICHA * s::POR_PAGINA];
        let (n, _) = pedir(&p, &mut r, 3_000_000);
        if n < 8 {
            break;
        }
        let van = u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize;
        for i in 0..van.min(s::POR_PAGINA) {
            let f = &r[8 + s::FICHA * i..8 + s::FICHA * (i + 1)];
            let uid = u32::from_le_bytes(f[8..12].try_into().unwrap());
            if uid >= s::PRIMERA_PERSONA && uid != 1001 {
                ajenos += 1;
            }
            if cuantos < vistos.len() {
                vistos[cuantos] = u64::from_le_bytes(f[0..8].try_into().unwrap());
                cuantos += 1;
            }
        }
        if van == 0 || cuantos >= vistos.len() {
            break;
        }
    }
    // Matar a ciegas lo que no ve: los números de `inicio` y los de lo que lanza `sesiones` (ahí está el de Ana). Nada
    // muere, y todo da "no existe": ni se sabe qué hay.
    let (mut probados, mut muertos, mut revelados) = (0, 0, 0);
    for pid in (1u64..64).chain(0x1_0000..0x1_0040) {
        if vistos[..cuantos].contains(&pid) {
            continue;
        }
        let mut p = [0u8; 12];
        p[0..4].copy_from_slice(&s::MATAR.to_le_bytes());
        p[4..12].copy_from_slice(&pid.to_le_bytes());
        let r = estado(&p);
        probados += 1;
        if r == s::BIEN {
            muertos += 1;
        } else if r != s::NO_EXISTE {
            revelados += 1;
        }
    }
    let procesos = cuantos > 0 && ajenos == 0 && muertos == 0 && revelados == 0;
    println!("  [cambio] Carla: ve {} procesos, {} de otra persona; matar a ciegas {}: {} muertos, {} dicen que existen: {}",
        cuantos, ajenos, probados, muertos, revelados, if procesos { "bien" } else { "MAL" });
    // Ser Ana ante `sesiones`: solo `guardian` puede.
    let entrar = con_textos(s::ENTRAR, b"Ana", b"clave de Ana");
    let volver = con_textos(s::VOLVER, b"Ana", b"clave de Ana");
    let salir = con_textos(s::SALIR, b"Ana", b"");
    let mut p = [0u8; 32];
    p[0..4].copy_from_slice(&s::LANZAR.to_le_bytes());
    p[4..8].copy_from_slice(&14u32.to_le_bytes());
    p[8..12].copy_from_slice(&1000u32.to_le_bytes());
    p[12..26].copy_from_slice(b"Programas/hola");
    let lanzar = estado(&p[..26]);
    let como_ana = entrar == a::PROTEGIDO && volver == a::PROTEGIDO && salir == a::PROTEGIDO && lanzar == s::PROTEGIDO;
    println!("  [cambio] Carla, como Ana: entrar {}, volver {}, salir {}, lanzar {}: {}", entrar, volver, salir, lanzar, if como_ana { "bien" } else { "MAL" });
    // Su volumen, aun con la contraseña buena: abrir, ver y cerrar volúmenes no es cosa de un programa.
    let clave = b"clave de Ana";
    arch.ventana()[..clave.len()].copy_from_slice(clave);
    let abrir = arch.orden(a::ABRIR_VOLUMEN, 0, 0, clave.len() as u64, b"Ana").0;
    arch.ventana()[..clave.len()].fill(0);
    let ver = arch.orden(a::VOLUMEN, 0, 0, 0, b"Ana").0;
    let cerrar = arch.orden(a::CERRAR_VOLUMEN, 0, 0, 0, b"Ana").0;
    // Y su fichero: por su `Home:` (el volumen de Carla), subiendo, por el disco y por raíces que no tiene.
    let caminos: [(u32, &[u8]); 5] = [
        (home as u32, b"testigo.txt"),
        (home as u32, b"../testigo.txt"),
        (home as u32, b"../Ana/testigo.txt"),
        (1, b"Ana/testigo.txt"),
        (1, b"Ana"),
    ];
    let mut llega = 0;
    for (asa, camino) in caminos {
        let (e, h, _) = arch.orden(a::BUSCAR, asa, 0, 0, camino);
        if e == a::BIEN {
            llega += 1;
            arch.orden(a::CERRAR, h, 0, 0, b"");
        }
    }
    for asa in 3..a::RAICES {
        let (e, h, _) = arch.orden(a::BUSCAR, asa, 0, 0, b"testigo.txt");
        if e == a::BIEN {
            llega += 1;
            arch.orden(a::CERRAR, h, 0, 0, b"");
        }
    }
    let ficheros = abrir == a::PROTEGIDO && ver == a::PROTEGIDO && cerrar == a::PROTEGIDO && llega == 0;
    println!("  [cambio] Carla, lo de Ana: abrir su volumen {}, verlo {}, cerrarlo {}; su fichero, por {} caminos: {}",
        abrir, ver, cerrar, llega, if ficheros { "bien" } else { "MAL" });
    let bien = delante == 1 && procesos && como_ana && ficheros;
    println!("  [cambio] Carla delante ({}), con Ana detrás: nada de Ana a su alcance: {}", delante, if bien { "bien" } else { "MAL" });
    bien
}
