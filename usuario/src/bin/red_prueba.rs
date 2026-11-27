//! La prueba de punta a punta de la red (S13), solo en `virt`. Llaves: 1 el canal "red" y 2 el canal "red-escuchar" (el
//! permiso de escuchar).
//!
//! Espera a que el servicio tenga dirección (DHCP) y prueba, contra el eco del PC (`tools/red_eco.py`, que QEMU lanza
//! por cada conexión a 10.0.2.100:7):
//! - que hacerse pasar por el controlador o tocar conexiones ajenas se rechaza;
//! - con el glotón (`red-gloton`) en su tope de conexiones, el eco de 4 MB al azar vuelve igual; mide el caudal y el
//!   tiempo de ida y vuelta;
//! - un puerto cerrado del PC da `RECHAZADA`;
//! - tras la basura que manda el PC (`tools/red_basura.py`: tramas cortas, largas y rotas), el controlador y el
//!   servicio siguen.
//!
//! Segunda parte, contra el vecino del PC (`tools/red_vecino.py`, 10.0.2.53) y quien entra desde fuera
//! (`tools/red_entra.py`), sin Internet: UDP (eco, el tope de la cola, una cabecera imposible), nombres (caché, alias,
//! `hosts`, respuestas falsas, nombres malos, un servidor mudo), escuchar por TCP y por UDP con el cortafuegos cerrado
//! de serie, IPv6 por SLAAC y la hora por SNTP.

#![no_std]
#![no_main]

use dashos::println;
use dashos::protocolo::red as p;
use dashos::red_cliente::{estado, resolver, Conexion, Oido, Udp};
use dashos::sntp;

dashos::programa!(principal);

const SERVICIO: u32 = 1;
/// El canal "red-escuchar": el permiso de escuchar.
const ESCUCHAR: u32 = 2;
const VA: usize = 0x50_0000_0000;
/// Otros sitios para proyectar, uno por zócalo abierto a la vez.
const VA2: usize = VA + (4 << 20);
const VA3: usize = VA + (8 << 20);
/// El vecino del PC (`tools/red_vecino.py`) y sus puertos.
const VECINO: [u8; 4] = [10, 0, 2, 53];
const DNS_VECINO: ([u8; 4], u16) = (VECINO, 10053);
const P_ECO: u16 = 4007;
const P_RAFAGA: u16 = 4009;
const P_LLAMA: u16 = 4011;
/// El resolvedor del sistema.
const SISTEMA: ([u8; 4], u16) = ([0; 4], 0);
const ECO: [u8; 4] = [10, 0, 2, 100];
const PUERTO_ECO: u16 = 7;

/// xorshift64: bytes al azar, repetibles desde la semilla.
struct Azar {
    s: u64,
    v: u64,
    quedan: u32,
}

impl Azar {
    fn nuevo(semilla: u64) -> Self {
        Azar { s: semilla | 1, v: 0, quedan: 0 }
    }

    fn byte(&mut self) -> u8 {
        if self.quedan == 0 {
            self.s ^= self.s << 13;
            self.s ^= self.s >> 7;
            self.s ^= self.s << 17;
            self.v = self.s;
            self.quedan = 8;
        }
        let b = self.v as u8;
        self.v >>= 8;
        self.quedan -= 1;
        b
    }
}

/// Manda `total` bytes al azar por `c` y comprueba que vuelven igual. Devuelve los µs que tardó, o un texto de fallo.
fn eco(c: &Conexion, total: usize, semilla: u64) -> Result<u64, &'static str> {
    let (mut gen_env, mut gen_ver) = (Azar::nuevo(semilla), Azar::nuevo(semilla));
    let mut pend = [0u8; 16384];
    let (mut ini, mut fin) = (0usize, 0usize);
    let (mut generados, mut recibidos) = (0usize, 0usize);
    let mut rbuf = [0u8; 16384];
    let t0 = dashos::ahora_us();
    let limite = t0 + 180_000_000;
    while recibidos < total {
        let mut progreso = false;
        if ini == fin && generados < total {
            let n = pend.len().min(total - generados);
            for b in pend[..n].iter_mut() {
                *b = gen_env.byte();
            }
            generados += n;
            ini = 0;
            fin = n;
        }
        if ini < fin {
            let n = c.enviar(&pend[ini..fin]);
            ini += n;
            progreso |= n > 0;
        }
        let n = c.recibir(&mut rbuf);
        if n > 0 {
            progreso = true;
            if recibidos + n > total {
                return Err("vuelve más de lo enviado");
            }
            for &b in &rbuf[..n] {
                if b != gen_ver.byte() {
                    return Err("vuelve otra cosa");
                }
            }
            recibidos += n;
        }
        if !progreso {
            if c.fase() != p::ABIERTA {
                return Err("la conexión se cerró a medias");
            }
            if dashos::ahora_us() > limite {
                return Err("no acaba");
            }
            c.esperar(true, ini < fin, 1_000_000);
        }
    }
    Ok(dashos::ahora_us() - t0)
}

fn ip(w: u32) -> [u8; 4] {
    w.to_le_bytes()
}

fn veredicto(bien: bool) -> &'static str {
    if bien { "bien" } else { "MAL" }
}

/// Espera un datagrama en `z` como mucho `plazo_us`: su largo, de qué dirección (IPv4) y de qué puerto.
fn recibir(z: &Udp, buf: &mut [u8], plazo_us: u64) -> Option<(usize, [u8; 4], u16)> {
    let fin = dashos::ahora_us() + plazo_us;
    loop {
        if let Some((n, d, _, puerto)) = z.recibir(buf) {
            return Some((n.min(buf.len()), [d[0], d[1], d[2], d[3]], puerto));
        }
        let ahora = dashos::ahora_us();
        if ahora >= fin || z.fase() != p::ABIERTA {
            return None;
        }
        z.esperar(fin - ahora);
    }
}

/// La dirección IPv4 de `nombre` según el vecino, o el error.
fn a(nombre: &[u8]) -> Result<[u8; 4], i32> {
    resolver(SERVICIO, nombre, p::IPV4, DNS_VECINO, 12_000_000).map(|d| d.v4())
}

/// S13, segunda parte: UDP, nombres, escuchar, IPv6 y la hora, contra el vecino del PC.
fn segunda_parte() {
    let tiradas = |e: &[u32; p::E_PALABRAS]| e[p::E_DNS_TIRADAS];
    let e0 = estado(SERVICIO, 2_000_000).unwrap_or([0; p::E_PALABRAS]);

    // UDP: el eco del vecino. Antes del eco, el vecino manda lo mismo haciéndose pasar por otra dirección: sin el
    // permiso de escuchar, eso no llega (se cuenta como perdido).
    let mut buf = [0u8; 2048];
    let Ok(z) = Udp::abrir(SERVICIO, 0, VA, 16 * 1024, 5_000_000) else {
        println!("  [red-prueba] UDP: MAL, no se abre un zócalo");
        return;
    };
    let texto = b"eco de DashOS por UDP";
    let puesto = z.enviar4(texto, VECINO, P_ECO);
    let vuelta = recibir(&z, &mut buf, 5_000_000);
    let igual = matches!(vuelta, Some((n, VECINO, P_ECO)) if &buf[..n] == texto);
    println!(
        "  [red-prueba] UDP, eco con el vecino: {} (vuelve igual: {}; lo que llega de otra dirección, sin pedirlo, se tira: {} perdido)",
        veredicto(puesto && igual && z.perdidos() == 1), igual, z.perdidos()
    );
    // El tope de la cola: 200 datagramas de 1200 bytes de golpe, sin leer. En el anillo de 16 KB caben 13; el resto se
    // pierde: lo que llega al zócalo con el anillo lleno lo cuenta él; lo demás no pasa de las colas de antes (las 16
    // del zócalo en la pila y las ranuras de la tarjeta, que lo cuenta como "sin sitio"). Después, el eco sigue.
    let antes = z.perdidos();
    let sin_sitio = estado(SERVICIO, 2_000_000).map(|e| e[p::E_SIN_SITIO]).unwrap_or(0);
    z.enviar4(b"rafaga 200", VECINO, P_RAFAGA);
    dashos::dormir_us(1_500_000);
    let (mut llegan, mut orden, mut ultimo) = (0u32, true, None);
    while let Some((n, _, _)) = recibir(&z, &mut buf, 1) {
        let numero = u32::from_le_bytes(buf[..4].try_into().unwrap());
        orden &= n == 1200 && ultimo.is_none_or(|u| numero > u);
        ultimo = Some(numero);
        llegan += 1;
    }
    let perdidos = z.perdidos() - antes;
    z.enviar4(texto, VECINO, P_ECO);
    let sigue = matches!(recibir(&z, &mut buf, 5_000_000), Some((n, VECINO, P_ECO)) if &buf[..n] == texto);
    let sin_sitio = estado(SERVICIO, 2_000_000).map(|e| e[p::E_SIN_SITIO]).unwrap_or(0) - sin_sitio;
    println!(
        "  [red-prueba] UDP, 200 datagramas de golpe sin leer: {} (llegan {} en orden, el tope de la cola; perdidos en el zócalo, {}; sin sitio en la tarjeta, {}; después el eco sigue: {})",
        veredicto((1..=13).contains(&llegan) && orden && sigue), llegan, perdidos, sin_sitio, sigue
    );
    // Hostil: una cabecera imposible en el anillo (una familia que no hay). El servicio da el zócalo por roto y sigue.
    z.enviar(b"x", [0; 16], 9, 1);
    let t = dashos::ahora_us();
    while z.fase() == p::ABIERTA && dashos::ahora_us() - t < 3_000_000 {
        dashos::dormir_us(20_000);
    }
    let roto = z.fase();
    z.cerrar();
    // Y sin el permiso de escuchar (por el canal "red"), ni un puerto elegido ni esperar a que entren.
    let puerto_elegido = Udp::abrir(SERVICIO, 40123, VA, 4096, 5_000_000).err();
    let entrar = Oido::escuchar(SERVICIO, 7777, VA, 4096, 5_000_000).err();
    println!(
        "  [red-prueba] UDP, una cabecera imposible: {} (el zócalo queda roto: {}); escuchar sin el permiso: {} (UDP {:?}, TCP {:?})",
        veredicto(roto == p::MAL_FORMADO), roto, veredicto(puerto_elegido == Some(p::SIN_PERMISO) && entrar == Some(p::SIN_PERMISO)), puerto_elegido, entrar
    );

    // Nombres. La dirección de bien.prueba cuenta las preguntas que le llegan al vecino y vale 3 s: la primera vez viene
    // de la red; la segunda, de la caché (la misma); pasados 3 s, otra vez de la red (la siguiente).
    let pedir = |nombre: &[u8], familia: u32| resolver(SERVICIO, nombre, familia, DNS_VECINO, 12_000_000);
    let primera = pedir(b"bien.prueba", p::IPV4).map(|d| (d.v4(), d.origen));
    let segunda = pedir(b"BIEN.prueba.", p::IPV4).map(|d| (d.v4(), d.origen));
    let seis = pedir(b"bien.prueba", p::IPV6).map(|d| (d.dirs[0][0], d.dirs[0][1], d.dirs[0][15]));
    dashos::dormir_us(3_500_000);
    let tercera = pedir(b"bien.prueba", p::IPV4).map(|d| (d.v4(), d.origen));
    let alias = a(b"alias.prueba");
    let cache = primera == Ok(([192, 0, 2, 1], p::DE_RED)) && segunda == Ok(([192, 0, 2, 1], p::DE_CACHE)) && tercera == Ok(([192, 0, 2, 2], p::DE_RED));
    println!(
        "  [red-prueba] nombres: {} (bien.prueba: {:?} de la red, {:?} de la caché y, vencida, {:?}; AAAA {:?}; un alias: {:?})",
        veredicto(cache && seis == Ok((0x20, 0x01, 1)) && alias == Ok([192, 0, 2, 77])),
        primera.map(|x| x.0), segunda.map(|x| x.0), tercera.map(|x| x.0), seis, alias
    );
    // `hosts` (Prefs/Red/hosts del disco), localhost y una dirección escrita: sin preguntar a nadie.
    let sistema = |nombre: &[u8], familia: u32| resolver(SERVICIO, nombre, familia, SISTEMA, 12_000_000);
    let de_hosts = sistema(b"vecino.prueba", p::IPV4).map(|d| (d.v4(), d.origen));
    let de_hosts6 = sistema(b"vecino.prueba", p::IPV6).map(|d| (d.dirs[0][0], d.dirs[0][15], d.origen));
    let local = sistema(b"localhost", p::IPV4).map(|d| d.v4());
    let escrita = sistema(b"10.1.2.3", p::IPV4).map(|d| d.v4());
    let solo_v4 = sistema(b"hora.prueba", p::IPV6).err();
    println!(
        "  [red-prueba] hosts: {} (vecino.prueba: {:?} y {:?}; localhost: {:?}; una dirección escrita: {:?}; sin IPv6 en hosts: {:?})",
        veredicto(de_hosts == Ok((VECINO, p::DE_HOSTS)) && de_hosts6 == Ok((0xfe, 0x53, p::DE_HOSTS)) && local == Ok([127, 0, 0, 1]) && escrita == Ok([10, 1, 2, 3]) && solo_v4 == Some(p::SIN_NOMBRE)),
        de_hosts.map(|x| x.0), de_hosts6.map(|x| (x.0, x.1)), local, escrita, solo_v4
    );
    // Hostil: envenenar. Antes de la respuesta buena, el vecino manda otra con otro identificador; la misma desde otra
    // dirección, desde otro puerto y desde el DNS de QEMU; la respuesta a otra pregunta; una con un nombre comprimido
    // en bucle; y una cortada. Todas llevan 6.6.6.6: tiene que salir la buena, y las falsas, contarse.
    let antes = estado(SERVICIO, 2_000_000).map(|e| tiradas(&e)).unwrap_or(0);
    let venenos = [a(b"falsa-id.prueba"), a(b"otro-origen.prueba"), a(b"otra-pregunta.prueba"), a(b"bucle.prueba"), a(b"corta.prueba")];
    let buenas = venenos.iter().enumerate().all(|(i, r)| *r == Ok([192, 0, 2, 20 + i as u8]));
    let falsas = estado(SERVICIO, 2_000_000).map(|e| tiradas(&e)).unwrap_or(0) - antes;
    println!(
        "  [red-prueba] nombres, respuestas falsas (otro identificador, otro origen, otra pregunta, bucle, cortada): {} (salen las buenas: {}; {} falsas tiradas)",
        veredicto(buenas && falsas >= 7), buenas, falsas
    );
    // Truncada, un nombre que no existe y veinte direcciones (se dan 8).
    let truncada = a(b"truncada.prueba").err();
    let no_existe = a(b"noexiste.prueba").err();
    let muchas = pedir(b"muchas.prueba", p::IPV4).map(|d| d.n);
    // Nombres malos: no salen a la red. Y uno de 240 bytes, el más largo que cabe, sí.
    let mut largo = [b'a'; 240];
    for i in [60, 120, 180, 230] {
        largo[i] = b'.';
    }
    largo[231..240].copy_from_slice(b"ok.prueba");
    let el_largo = a(&largo);
    let malos = [a(&[b'a'; 64]).err(), a(b"a..b").err(), a(b"con espacio.prueba").err(), a(b"").err(), a(&[b'a'; 241]).err(), a("ñu.prueba".as_bytes()).err()];
    // Una petición que miente en el largo del nombre, a mano.
    let mentira = {
        let (lee, escribe) = dashos::canal_crear();
        let copia = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
        let mut m = [0u8; 24];
        for (i, v) in [p::RESOLVER, p::IPV4, 0, 5000].into_iter().enumerate() {
            m[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        dashos::canal_escribir(SERVICIO, &m, &[copia as u32]);
        let mut r = [0u8; 64];
        let (n, _, _) = dashos::canal_leer_plazo(lee, &mut r, &mut [], 3_000_000);
        dashos::cerrar(lee);
        dashos::cerrar(escribe);
        if n >= 8 { i32::from_le_bytes(r[4..8].try_into().unwrap()) } else { 1 }
    };
    println!(
        "  [red-prueba] nombres, raros: {} (truncada: {:?}; no existe: {:?}; veinte direcciones: {:?}; uno de 240 bytes: {:?}; malos: {:?}; largo que miente: {})",
        veredicto(truncada == Some(p::TRUNCADA) && no_existe == Some(p::SIN_NOMBRE) && muchas == Ok(8) && el_largo == Ok([192, 0, 2, 30]) && malos.iter().all(|m| *m == Some(p::MAL_FORMADO)) && mentira == p::MAL_FORMADO),
        truncada, no_existe, muchas, el_largo, malos, mentira
    );
    // Un servidor que no contesta: tres intentos (1, 2 y 4 s) y se da por perdido. Mientras, lo demás sigue.
    let t = dashos::ahora_us();
    let mudo = a(b"mudo.prueba").err();
    let tardo = (dashos::ahora_us() - t) / 1000;
    println!("  [red-prueba] nombres, un servidor mudo: {} ({:?} a los {} ms, tras tres intentos)", veredicto(mudo == Some(p::SIN_RESPUESTA) && (6_000..11_000).contains(&tardo)), mudo, tardo);
    // Al servidor de nombres de QEMU (10.0.2.3, el del DHCP) no se le pregunta: reenvía al del PC, y de ahí a Internet.
    // Ninguna prueba sale fuera (`docs/bitacoras/red.md`, 2026-11-16); además, QEMU va con `restrict=on`.

    // Escuchar, con el cortafuegos cerrado de serie. Desde el PC, `red_entra.py` lleva intentando entrar por el 7777
    // desde que arrancó QEMU: mientras nadie escucha, el cortafuegos tira sus intentos (se ve en la cuenta).
    let t = dashos::ahora_us();
    let mut cortados = 0;
    while cortados == 0 && dashos::ahora_us() - t < 20_000_000 {
        cortados = estado(SERVICIO, 2_000_000).map(|e| e[p::E_CORTADOS_TCP]).unwrap_or(0);
        if cortados == 0 {
            dashos::dormir_us(200_000);
        }
    }
    let mut entra = (false, [0u8; 4], false, false);
    match Oido::escuchar(ESCUCHAR, 7777, VA, 4096, 5_000_000) {
        Err(e) => println!("  [red-prueba] escuchar en el 7777 con el permiso: no se puede ({})", e),
        Ok(mut oido) => match oido.aceptar(30_000_000) {
            Ok(Some(c)) => {
                let (de, _, _) = c.remoto();
                // La línea que manda, y su eco.
                let (mut linea, mut n) = ([0u8; 64], 0);
                let t = dashos::ahora_us();
                while !linea[..n].contains(&b'\n') && n < linea.len() && dashos::ahora_us() - t < 5_000_000 {
                    n += c.recibir(&mut linea[n..]);
                    if !linea[..n].contains(&b'\n') {
                        c.esperar(true, false, 500_000);
                    }
                }
                let devuelto = c.enviar(&linea[..n]) == n;
                // El otro lado cierra al ver su eco.
                let t = dashos::ahora_us();
                while c.fase() == p::ABIERTA && dashos::ahora_us() - t < 5_000_000 {
                    c.esperar(true, false, 200_000);
                }
                entra = (true, [de[0], de[1], de[2], de[3]], &linea[..n] == b"hola desde fuera\n" && devuelto, c.fase() == p::FIN);
                c.cerrar();
            }
            Ok(None) => oido.cerrar(),
            Err(e) => println!("  [red-prueba] escuchar en el 7777: {}", e),
        },
    }
    println!(
        "  [red-prueba] escuchar por TCP: {} (antes, {} intentos de fuera cortados por el cortafuegos; con el permiso, entra {}.{}.{}.{}: {}; llega su línea y vuelve su eco: {}; cierra: {})",
        veredicto(cortados > 0 && entra.0 && entra.2 && entra.3), cortados, entra.1[0], entra.1[1], entra.1[2], entra.1[3], entra.0, entra.2, entra.3
    );
    // Por UDP: con el permiso, un puerto elegido recibe de cualquiera. El vecino, a petición, manda desde otra
    // dirección un datagrama a ese puerto y otro a uno que nadie tiene: el primero llega; el segundo lo tira el
    // cortafuegos sin contestar (el vecino dice si DashOS mandó algo a esa dirección). El mismo puerto, dos veces, no.
    let antes = estado(SERVICIO, 2_000_000).map(|e| e[p::E_CORTADOS_UDP]).unwrap_or(0);
    match (Udp::abrir(ESCUCHAR, 40123, VA2, 4096, 5_000_000), Udp::abrir(SERVICIO, 0, VA3, 4096, 5_000_000)) {
        (Ok(abierto), Ok(pide)) => {
            let repetido = Udp::abrir(ESCUCHAR, 40123, VA, 4096, 5_000_000).err();
            pide.enviar4(b"llama 40123 40124", VECINO, P_LLAMA);
            let llega = matches!(recibir(&abierto, &mut buf, 5_000_000), Some((n, [10, 0, 2, 66], P_LLAMA)) if &buf[..n] == b"llaman a la puerta 40123");
            let informe = recibir(&pide, &mut buf, 5_000_000).map(|(n, _, _)| n);
            let callado = informe.is_some_and(|n| &buf[..n] == b"contestado al otro: 0");
            let cortados = estado(SERVICIO, 2_000_000).map(|e| e[p::E_CORTADOS_UDP]).unwrap_or(0) - antes;
            println!(
                "  [red-prueba] escuchar por UDP: {} (con el permiso, al 40123 llega lo de cualquiera: {}; al 40124, que nadie tiene, lo corta el cortafuegos ({}) sin contestar: {}; el mismo puerto dos veces: {:?})",
                veredicto(llega && callado && cortados >= 1 && repetido == Some(p::OCUPADO)), llega, cortados, callado, repetido
            );
            abierto.cerrar();
            pide.cerrar();
        }
        (x, y) => println!("  [red-prueba] escuchar por UDP: MAL, no se abren los zócalos ({:?}, {:?})", x.err(), y.err()),
    }

    // IPv6: la dirección global, del prefijo que anuncia QEMU (fec0::/64) y la MAC. Y una conexión: el puerto 1 del
    // PC por IPv6 (fec0::2) está cerrado; si IPv6 va de punta a punta, la rechaza al momento (si no, tardaría 10 s).
    let t = dashos::ahora_us();
    let mut e = estado(SERVICIO, 2_000_000).unwrap_or(e0);
    while e[p::E_IP6] == 0 && dashos::ahora_us() - t < 20_000_000 {
        dashos::dormir_us(500_000);
        e = estado(SERVICIO, 2_000_000).unwrap_or(e);
    }
    let mut dir = [0u8; 16];
    for i in 0..4 {
        dir[4 * i..4 * i + 4].copy_from_slice(&e[p::E_IP6_DIR + i].to_le_bytes());
    }
    let esperada = [0xfe, 0xc0, 0, 0, 0, 0, 0, 0, 0x50, 0x54, 0, 0xff, 0xfe, 0x12, 0x34, 0x56];
    let mut pc = [0u8; 16];
    pc[0] = 0xfe;
    pc[1] = 0xc0;
    pc[15] = 2;
    let t = dashos::ahora_us();
    let cerrado = Conexion::conectar6(SERVICIO, pc, 1, VA, 4096, 15_000_000).err();
    let tardo = (dashos::ahora_us() - t) / 1000;
    println!(
        "  [red-prueba] IPv6: {} (dirección por SLAAC fec0::5054:ff:fe12:3456: {}; un puerto cerrado del PC por IPv6: {:?} a los {} ms)",
        veredicto(e[p::E_IP6] == 1 && dir == esperada && cerrado == Some(p::RECHAZADA) && tardo < 8_000), e[p::E_IP6] == 1 && dir == esperada, cerrado, tardo
    );

    // La hora: lo que dice el vecino por SNTP (tras sus tres respuestas que no valen) y el reloj de DashOS.
    match Udp::abrir(SERVICIO, 0, VA, 4096, 5_000_000) {
        Err(e) => println!("  [red-prueba] la hora por SNTP: MAL, no se abre un zócalo ({})", e),
        Ok(z) => {
            let mut numero = [0u8; 8];
            dashos::azar(&mut numero);
            let numero = u64::from_le_bytes(numero);
            let ida = dashos::ahora_ns();
            z.enviar4(&sntp::peticion(numero), VECINO, sntp::PUERTO);
            let (mut falsas, mut difiere) = (0, None);
            while let Some((n, _, _)) = recibir(&z, &mut buf, 3_000_000) {
                match sntp::respuesta(&buf[..n], numero) {
                    Ok(h) => {
                        let ns = sntp::al_recibir(&h, dashos::ahora_ns() - ida);
                        difiere = Some(ns.abs_diff(dashos::hora_ns()) / 1_000_000);
                        break;
                    }
                    Err(_) => falsas += 1,
                }
            }
            z.cerrar();
            println!(
                "  [red-prueba] la hora por SNTP: {} (el vecino y el reloj de DashOS difieren {:?} ms; {} respuestas que no valen, tiradas)",
                veredicto(difiere.is_some_and(|d| d < 5_000) && falsas == 3), difiere, falsas
            );
        }
    }
}

fn principal() -> u64 {
    // La dirección: el servicio la tiene cuando DHCP acaba.
    let t0 = dashos::ahora_us();
    let e = loop {
        match estado(SERVICIO, 2_000_000) {
            Ok(e) if e[p::E_CONFIGURADA] == 1 => break e,
            _ if dashos::ahora_us() - t0 > 60_000_000 => {
                println!("  [red-prueba] la red no tiene dirección tras 60 s");
                return 1;
            }
            _ => dashos::dormir_us(200_000),
        }
    };
    let a = ip(e[p::E_IP]);
    println!("  [red-prueba] la red tiene dirección {}.{}.{}.{} ({} ms desde que empecé a mirar)", a[0], a[1], a[2], a[3], (dashos::ahora_us() - t0) / 1000);

    // Lo que no le toca: decir que es el controlador, avisar y cerrar conexiones que no son suyas, órdenes raras.
    let antes = e[p::E_RECHAZOS];
    let (lee, escribe) = dashos::canal_crear();
    let mem = dashos::memoria_crear(4096);
    let falsa = dashos::llave_duplicar(mem, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let canal = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let mut tarjeta = [0u8; 12];
    tarjeta[..4].copy_from_slice(&p::TARJETA.to_le_bytes());
    // Con la cola del servicio llena (16 mensajes; la máquina cargada al arrancar), `canal_escribir` da -8 y el mensaje no
    // llega: se reintenta un rato, como en `mal` (salían 12 rechazos en vez de 14, una vez de cuatro).
    let mandar = |m: &[u8], ll: &[u32]| {
        for _ in 0..2000 {
            if dashos::canal_escribir(SERVICIO, m, ll) != -8 {
                return;
            }
            dashos::dormir_us(1_000);
        }
    };
    mandar(&tarjeta, &[falsa as u32, canal as u32]);
    for orden in [p::AVISO, p::CERRAR] {
        for id in [0u32, 1, 0x100, 0x201, u32::MAX] {
            let mut m = [0u8; 8];
            m[..4].copy_from_slice(&orden.to_le_bytes());
            m[4..].copy_from_slice(&id.to_le_bytes());
            mandar(&m, &[]);
        }
    }
    mandar(&999u32.to_le_bytes(), &[]);
    mandar(&[], &[]);
    dashos::cerrar(lee);
    dashos::cerrar(escribe);
    dashos::cerrar(mem);
    // Mal formados: anillos de más o que no son potencia de dos, una familia que no hay. Y sin canal para contestar.
    let mal = |familia: u32, tam: u32| -> i32 {
        let (lee, escribe) = dashos::canal_crear();
        let ee = dashos::llave_duplicar(escribe, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
        let pet = [p::CONECTAR, familia, PUERTO_ECO as u32, tam, u32::from_le_bytes(ECO), 0, 0, 0];
        let bytes = unsafe { core::slice::from_raw_parts(pet.as_ptr() as *const u8, 32) };
        // Tras la ráfaga de arriba, la cola del servicio puede estar llena (con el PC cargado): se reintenta un rato,
        // en vez de dar por perdida la petición (salía 1, sin respuesta, en vez de MAL_FORMADO).
        for _ in 0..2000 {
            if dashos::canal_escribir(SERVICIO, bytes, &[ee as u32]) != -8 {
                break;
            }
            dashos::dormir_us(1_000);
        }
        let mut r = [0u8; 16];
        let mut ll = [0u32; 8];
        let (n, _, _) = dashos::canal_leer_plazo(lee, &mut r, &mut ll, 2_000_000);
        dashos::cerrar(lee);
        dashos::cerrar(escribe);
        if n >= 12 { i32::from_le_bytes(r[8..12].try_into().unwrap()) } else { 1 }
    };
    let malos = [mal(p::IPV4, 1 << 20), mal(p::IPV4, 3000), mal(5, 4096)];
    let pet = [p::CONECTAR, p::IPV4, PUERTO_ECO as u32, 4096, u32::from_le_bytes(ECO), 0, 0, 0];
    dashos::canal_escribir(SERVICIO, unsafe { core::slice::from_raw_parts(pet.as_ptr() as *const u8, 32) }, &[]);
    let despues = estado(SERVICIO, 2_000_000).map(|e| e[p::E_RECHAZOS]).unwrap_or(0);
    let bien = despues >= antes + 13 && malos == [p::MAL_FORMADO, p::MAL_FORMADO, p::NO_SOPORTADA];
    println!(
        "  [red-prueba] hacerse pasar por el controlador o tocar lo ajeno: {} rechazos; mal formados {:?}: {}",
        despues - antes, malos, if bien { "bien" } else { "MAL" }
    );

    // Con el glotón en su tope: los demás siguen.
    let t1 = dashos::ahora_us();
    let mut tope = 0;
    while dashos::ahora_us() - t1 < 30_000_000 {
        tope = estado(SERVICIO, 2_000_000).map(|e| e[p::E_CONEXIONES]).unwrap_or(0);
        if tope >= p::POR_PROGRAMA as u32 {
            break;
        }
        dashos::dormir_us(100_000);
    }
    let Ok(c) = Conexion::conectar(SERVICIO, ECO, PUERTO_ECO, VA, p::ANILLO_MAX, 15_000_000) else {
        println!("  [red-prueba] no conecta con el eco del PC");
        return 2;
    };
    // Ida y vuelta: 64 bytes, 50 veces.
    let mut tiempos = [0u64; 50];
    let mut b = [0u8; 64];
    let mut azar = Azar::nuevo(0x1234_5678);
    for t in tiempos.iter_mut() {
        for x in b.iter_mut() {
            *x = azar.byte();
        }
        let ida = dashos::ahora_us();
        let mut puesto = 0;
        while puesto < 64 {
            puesto += c.enviar(&b[puesto..]);
        }
        let mut vuelto = [0u8; 64];
        let mut n = 0;
        while n < 64 && c.fase() == p::ABIERTA {
            n += c.recibir(&mut vuelto[n..]);
            if n < 64 {
                c.esperar(true, false, 1_000_000);
            }
        }
        *t = dashos::ahora_us() - ida;
        if vuelto != b {
            println!("  [red-prueba] ida y vuelta: vuelve otra cosa");
            return 3;
        }
    }
    tiempos.sort_unstable();
    println!("  [red-prueba] ida y vuelta de 64 bytes, 50 veces: mediana {} µs, mínimo {} µs, máximo {} µs", tiempos[25], tiempos[0], tiempos[49]);
    let mut semilla = [0u8; 8];
    dashos::azar(&mut semilla);
    let total = 4 << 20;
    match eco(&c, total, u64::from_le_bytes(semilla)) {
        Ok(us) => {
            // MB/s con dos decimales, en enteros: lo enviado (y lo vuelto es lo mismo).
            let centesimas = total as u64 * 100 / us.max(1);
            println!("  [red-prueba] eco de 4 MB al azar: vuelve igual: bien");
            println!("  [red-prueba] caudal: {}.{:02} MB/s de ida (y lo mismo de vuelta), {} ms", centesimas / 100, centesimas % 100, us / 1000);
        }
        Err(m) => {
            println!("  [red-prueba] eco de 4 MB: {}", m);
            return 4;
        }
    }
    println!("  [red-prueba] con el glotón en su tope ({} conexiones abiertas): {}", tope, if tope >= p::POR_PROGRAMA as u32 { "bien" } else { "MAL, no llegó" });
    c.cerrar();

    // Un puerto cerrado del PC (10.0.2.2 es el PC para QEMU): el otro lado contesta que no.
    match Conexion::conectar(SERVICIO, [10, 0, 2, 2], 1, VA, 4096, 15_000_000) {
        Err(p::RECHAZADA) => println!("  [red-prueba] puerto cerrado: rechazada: bien"),
        Err(e) => println!("  [red-prueba] puerto cerrado: {} (MAL)", e),
        Ok(c) => {
            println!("  [red-prueba] puerto cerrado: se abre (MAL)");
            c.cerrar();
        }
    }

    segunda_parte();

    // La basura del PC: el controlador tira lo corto y lo largo; la pila, lo roto. Después, el eco sigue.
    let t2 = dashos::ahora_us();
    let mut e = estado(SERVICIO, 2_000_000).unwrap_or([0; p::E_PALABRAS]);
    while (e[p::E_CORTAS] == 0 || e[p::E_LARGAS] == 0) && dashos::ahora_us() - t2 < 60_000_000 {
        dashos::dormir_us(500_000);
        e = estado(SERVICIO, 2_000_000).unwrap_or(e);
    }
    let otra = Conexion::conectar(SERVICIO, ECO, PUERTO_ECO, VA, 64 * 1024, 15_000_000);
    let sigue = match &otra {
        Ok(c) => eco(c, 256 * 1024, 77).is_ok(),
        Err(_) => false,
    };
    if let Ok(c) = otra {
        c.cerrar();
    }
    let e = estado(SERVICIO, 2_000_000).unwrap_or(e);
    println!(
        "  [red-prueba] tramas: {} recibidas, {} enviadas; tiradas: {} cortas, {} largas, {} sin sitio",
        e[p::E_RECIBIDAS], e[p::E_ENVIADAS], e[p::E_CORTAS], e[p::E_LARGAS], e[p::E_SIN_SITIO]
    );
    let bien = sigue && e[p::E_CORTAS] > 0 && e[p::E_LARGAS] > 0;
    println!("  [red-prueba] basura del PC tirada; eco después: {}", if bien { "bien" } else { "MAL" });
    0
}
