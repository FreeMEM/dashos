//! El lado del programa del servicio de red (S13): abrir una conexión TCP, enviar, recibir y cerrar, con los anillos
//! de `protocolo::red`. Sin hilos ni montón: cada conexión es su memoria compartida y un canal propio por el que el
//! servicio contesta y avisa. También resolver un nombre (`resolver`), UDP (`Udp`) y, con el permiso de escuchar,
//! esperar a que entren (`Oido`).

use crate::protocolo::red as p;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

pub struct Conexion {
    servicio: u32,
    id: u32,
    va: usize,
    tam: usize,
    memoria: u32,
    lee: u32,
    escribe: u32,
}

/// Las palabras de una orden en bytes little-endian, como las lee el servicio (también desde la caja Amiga, que es
/// big-endian: allí el orden de la memoria no es el del servicio). Como mucho 8 palabras.
fn palabras<'a>(p: &[u32], buf: &'a mut [u8; 32]) -> &'a [u8] {
    let n = p.len().min(8);
    for (i, v) in p[..n].iter().enumerate() {
        buf[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    &buf[..4 * n]
}

/// Lo leído del servicio (little-endian) en palabras de este procesador.
fn de_le(b: &mut [u32]) {
    b.iter_mut().for_each(|x| *x = u32::from_le(*x));
}

/// Manda `ESTADO` y devuelve sus palabras (`protocolo::red::E_*`), o el error.
pub fn estado(servicio: u32, plazo_us: u64) -> Result<[u32; p::E_PALABRAS], i64> {
    let (lee, escribe) = crate::canal_crear();
    let copia = crate::llave_duplicar(escribe, crate::ESCRIBIR | crate::TRANSFERIR, 0);
    let r = if copia <= 0 {
        Err(copia)
    } else {
        let e = crate::canal_escribir(servicio, palabras(&[p::ESTADO], &mut [0; 32]), &[copia as u32]);
        if e != 0 {
            crate::cerrar(copia as u32);
            Err(e)
        } else {
            let mut b = [0u32; p::E_PALABRAS];
            let mut ll = [0u32; 8];
            let bytes = unsafe { core::slice::from_raw_parts_mut(b.as_mut_ptr() as *mut u8, 4 * b.len()) };
            let (n, k, _) = crate::canal_leer_plazo(lee, bytes, &mut ll, plazo_us);
            de_le(&mut b);
            let k = if n >= 0 { k.min(ll.len()) } else { 0 };
            ll[..k].iter().for_each(|&l| {
                crate::cerrar(l);
            });
            if n < 0 {
                Err(n)
            } else if b[0] != p::R_ESTADO {
                Err(p::MAL_FORMADO as i64)
            } else {
                Ok(b)
            }
        }
    };
    crate::cerrar(lee);
    crate::cerrar(escribe);
    r
}

impl Conexion {
    /// Abre una conexión TCP a `ip:puerto` por el canal `servicio`, con anillos de `tam` bytes. La memoria de la
    /// conexión (64 bytes y los dos anillos), que da el servicio, se proyecta en `va`, que tiene que estar libre. Espera
    /// la respuesta como mucho `plazo_us`.
    pub fn conectar(servicio: u32, ip: [u8; 4], puerto: u16, va: usize, tam: usize, plazo_us: u64) -> Result<Conexion, i32> {
        let ipw = u32::from_le_bytes(ip);
        Self::pedir(servicio, &[p::CONECTAR, p::IPV4, puerto as u32, tam as u32, ipw, 0, 0, 0], va, tam, plazo_us)
    }

    /// Lo mismo, a una dirección IPv6.
    pub fn conectar6(servicio: u32, ip: [u8; 16], puerto: u16, va: usize, tam: usize, plazo_us: u64) -> Result<Conexion, i32> {
        let w = |i: usize| u32::from_le_bytes(ip[4 * i..4 * i + 4].try_into().unwrap());
        Self::pedir(servicio, &[p::CONECTAR, p::IPV6, puerto as u32, tam as u32, w(0), w(1), w(2), w(3)], va, tam, plazo_us)
    }

    /// Empieza una conexión TCP a `ip:puerto` sin esperar la respuesta (la caja Amiga: `connect` no puede parar a las
    /// demás tareas 68k): `Pendiente::mirar` dice cuándo está.
    pub fn empezar(servicio: u32, ip: [u8; 4], puerto: u16, va: usize, tam: usize) -> Result<Pendiente, i32> {
        let ipw = u32::from_le_bytes(ip);
        Self::empezar_pedir(servicio, &[p::CONECTAR, p::IPV4, puerto as u32, tam as u32, ipw, 0, 0, 0], va, tam)
    }

    fn empezar_pedir(servicio: u32, peticion: &[u32], va: usize, tam: usize) -> Result<Pendiente, i32> {
        let (lee, escribe) = crate::canal_crear();
        let c = Conexion { servicio, id: 0, va, tam, memoria: 0, lee, escribe };
        // Con la cola del servicio llena se reintenta un poco (50 ms), como `pedir`.
        c.mandar(peticion, crate::ahora_us() + 50_000)?;
        Ok(Pendiente { c: Some(c) })
    }

    /// Manda `peticion` (una orden que se contesta con `R_CONECTAR`) con un canal nuevo para la respuesta y la espera.
    fn pedir(servicio: u32, peticion: &[u32], va: usize, tam: usize, plazo_us: u64) -> Result<Conexion, i32> {
        let (lee, escribe) = crate::canal_crear();
        let c = Conexion { servicio, id: 0, va, tam, memoria: 0, lee, escribe };
        let fin = crate::ahora_us() + plazo_us;
        c.mandar(peticion, fin)?;
        c.respuesta(fin)
    }

    /// Manda `peticion` con una copia del extremo de escritura de su canal. Si falla, suelta el canal.
    fn mandar(&self, peticion: &[u32], fin: u64) -> Result<(), i32> {
        let e = crate::llave_duplicar(self.escribe, crate::ESCRIBIR | crate::TRANSFERIR, 0);
        if e <= 0 {
            self.soltar();
            return Err(p::LIMITE);
        }
        // Con la cola del servicio llena (-8, E_LIMITE del kernel: el servicio va atrasado, pasa con el PC cargado), se
        // espera y se reintenta dentro del plazo. Antes salía en el acto como SIN_DIRECCION y `red-gloton` contaba 50
        // "no" seguidos que no eran el tope.
        loop {
            let r = crate::canal_escribir(self.servicio, palabras(peticion, &mut [0; 32]), &[e as u32]);
            if r == 0 {
                return Ok(());
            }
            if r != -8 || crate::ahora_us() >= fin {
                crate::cerrar(e as u32);
                self.soltar();
                return Err(if r == -8 { p::LIMITE } else { p::SIN_DIRECCION });
            }
            crate::dormir_us(2_000);
        }
    }

    /// Espera `R_CONECTAR` hasta `fin` y proyecta la memoria que trae. Si no llega o es un error, suelta el canal.
    fn respuesta(self, fin: u64) -> Result<Conexion, i32> {
        let mut c = Some(self);
        loop {
            let ahora = crate::ahora_us();
            if ahora >= fin {
                c.take().unwrap().soltar();
                return Err(p::RECHAZADA);
            }
            if let Some(r) = Self::leer_respuesta(&mut c, fin - ahora) {
                return r;
            }
        }
    }

    /// Una lectura del canal de respuesta (como mucho `plazo_us`): con `R_CONECTAR`, la conexión (o el error, y el canal
    /// suelto); sin nada, `None` y `c` sigue.
    fn leer_respuesta(c: &mut Option<Conexion>, plazo_us: u64) -> Option<Result<Conexion, i32>> {
        let (va, tam, lee) = {
            let x = c.as_ref()?;
            (x.va, x.tam, x.lee)
        };
        let mut b = [0u32; 4];
        let mut ll = [0u32; 8];
        {
            let bytes = unsafe { core::slice::from_raw_parts_mut(b.as_mut_ptr() as *mut u8, 16) };
            let (n, k, _) = crate::canal_leer_plazo(lee, bytes, &mut ll, plazo_us.max(1));
            de_le(&mut b);
            let k = if n >= 0 { k.min(ll.len()) } else { 0 };
            if n >= 12 && b[0] == p::R_CONECTAR {
                let c = c.take().unwrap();
                if b[2] as i32 != p::BIEN || k != 1 {
                    ll[..k].iter().for_each(|&l| {
                        crate::cerrar(l);
                    });
                    c.soltar();
                    return Some(Err(if b[2] as i32 != p::BIEN { b[2] as i32 } else { p::MAL_FORMADO }));
                }
                let memoria = ll[0];
                let tamano = crate::memoria_tamano(memoria);
                if tamano < (p::DATOS + 2 * tam) as i64 || crate::memoria_proyectar(memoria, va, true) != 0 {
                    crate::cerrar(memoria);
                    c.soltar();
                    return Some(Err(p::MAL_FORMADO));
                }
                return Some(Ok(Conexion { id: b[1], memoria, ..c }));
            }
            ll[..k].iter().for_each(|&l| {
                crate::cerrar(l);
            });
        }
        None
    }

    /// El puerto de este lado y quién es el otro (TCP): su dirección (16 bytes; IPv4 en los 4 primeros), su familia
    /// (`IPV4` o `IPV6`) y su puerto.
    pub fn puerto(&self) -> u16 {
        self.palabra(p::LOCAL_PUERTO) as u16
    }

    pub fn remoto(&self) -> ([u8; 16], u32, u16) {
        let mut d = [0u8; 16];
        for i in 0..4 {
            d[4 * i..4 * i + 4].copy_from_slice(&self.palabra(p::REMOTO_DIR + i).to_le_bytes());
        }
        (d, self.palabra(p::REMOTO_FAMILIA), self.palabra(p::REMOTO_PUERTO) as u16)
    }

    fn soltar(&self) {
        if self.memoria != 0 {
            crate::memoria_quitar(self.va);
            crate::cerrar(self.memoria);
        }
        crate::cerrar(self.lee);
        crate::cerrar(self.escribe);
    }

    fn palabra(&self, i: usize) -> u32 {
        u32::from_le(unsafe { read_volatile((self.va + 4 * i) as *const u32) })
    }

    fn poner(&self, i: usize, v: u32) {
        unsafe { write_volatile((self.va + 4 * i) as *mut u32, v.to_le()) }
    }

    /// El número de la conexión en el servicio.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// `ABIERTA`, `FIN` o un error (`protocolo::red`).
    pub fn fase(&self) -> i32 {
        self.palabra(p::FASE) as i32
    }

    /// Si el servicio espera, se le avisa (tras mover un anillo).
    fn avisar(&self) {
        fence(Ordering::SeqCst);
        if self.palabra(p::SERVICIO_ESPERA) != 0 {
            self.poner(p::SERVICIO_ESPERA, 0);
            crate::canal_escribir(self.servicio, palabras(&[p::AVISO, self.id], &mut [0; 32]), &[]);
        }
    }

    /// Pone en el anillo de envío lo que quepa de `datos`; devuelve cuánto. No espera.
    pub fn enviar(&self, datos: &[u8]) -> usize {
        let cabeza = self.palabra(p::ENV_CABEZA);
        let ocupado = cabeza.wrapping_sub(self.palabra(p::ENV_COLA)) as usize;
        let n = datos.len().min(self.tam.saturating_sub(ocupado));
        if n == 0 {
            return 0;
        }
        let desde = cabeza as usize & (self.tam - 1);
        let primero = n.min(self.tam - desde);
        let base = self.va + p::DATOS;
        // Memoria compartida con el servicio: copia de bytes, sin más.
        unsafe {
            core::ptr::copy_nonoverlapping(datos.as_ptr(), (base + desde) as *mut u8, primero);
            core::ptr::copy_nonoverlapping(datos[primero..].as_ptr(), base as *mut u8, n - primero);
        }
        fence(Ordering::SeqCst);
        self.poner(p::ENV_CABEZA, cabeza.wrapping_add(n as u32));
        self.avisar();
        n
    }

    /// Saca del anillo de recepción lo que haya, hasta llenar `buf`; devuelve cuánto. No espera.
    pub fn recibir(&self, buf: &mut [u8]) -> usize {
        let cola = self.palabra(p::REC_COLA);
        let hay = (self.palabra(p::REC_CABEZA).wrapping_sub(cola) as usize).min(self.tam);
        let n = buf.len().min(hay);
        if n == 0 {
            return 0;
        }
        fence(Ordering::SeqCst);
        let desde = cola as usize & (self.tam - 1);
        let primero = n.min(self.tam - desde);
        let base = self.va + p::DATOS + self.tam;
        unsafe {
            core::ptr::copy_nonoverlapping((base + desde) as *const u8, buf.as_mut_ptr(), primero);
            core::ptr::copy_nonoverlapping(base as *const u8, buf[primero..].as_mut_ptr(), n - primero);
        }
        fence(Ordering::SeqCst);
        self.poner(p::REC_COLA, cola.wrapping_add(n as u32));
        self.avisar();
        n
    }

    /// Bytes esperando en el anillo de recepción y sitio en el de envío.
    pub fn pendiente(&self) -> (usize, usize) {
        let rec = (self.palabra(p::REC_CABEZA).wrapping_sub(self.palabra(p::REC_COLA)) as usize).min(self.tam);
        let env = (self.palabra(p::ENV_CABEZA).wrapping_sub(self.palabra(p::ENV_COLA)) as usize).min(self.tam);
        (rec, self.tam - env)
    }

    /// Duerme hasta que el servicio mueva algo (o `plazo_us`, 0 sin plazo). Antes de dormir mira otra vez: si ya hay
    /// algo que leer, sitio para enviar o la conexión ya no está abierta, vuelve.
    pub fn esperar(&self, quiere_leer: bool, quiere_enviar: bool, plazo_us: u64) {
        self.poner(p::CLIENTE_ESPERA, 1);
        fence(Ordering::SeqCst);
        let (rec, sitio) = self.pendiente();
        if (quiere_leer && rec > 0) || (quiere_enviar && sitio > 0) || self.fase() != p::ABIERTA {
            self.poner(p::CLIENTE_ESPERA, 0);
            return;
        }
        let mut b = [0u8; 16];
        let mut ll = [0u32; 8];
        let (n, k, _) = crate::canal_leer_plazo(self.lee, &mut b, &mut ll, plazo_us);
        let k = if n >= 0 { k.min(ll.len()) } else { 0 };
        ll[..k].iter().for_each(|&l| {
            crate::cerrar(l);
        });
    }

    /// Cierra: el servicio manda lo que quede en el anillo de envío y después cierra la conexión.
    pub fn cerrar(self) {
        crate::canal_escribir(self.servicio, palabras(&[p::CERRAR, self.id], &mut [0; 32]), &[]);
        self.soltar();
    }
}

/// Una conexión TCP (o un zócalo UDP) pedida sin esperar (`Conexion::empezar`, `Udp::empezar`).
pub struct Pendiente {
    c: Option<Conexion>,
}

impl Pendiente {
    /// Mira, sin esperar, si ya contestó el servicio: la conexión, el error, o `None` si aún no.
    pub fn mirar(&mut self) -> Option<Result<Conexion, i32>> {
        Conexion::leer_respuesta(&mut self.c, 1)
    }

    /// Ya no se quiere (si llega la conexión, el servicio la cierra al ver el canal suelto).
    pub fn abandonar(mut self) {
        if let Some(c) = self.c.take() {
            c.soltar();
        }
    }
}

/// Las direcciones de un nombre (`resolver`).
pub struct Direcciones {
    pub n: usize,
    /// De 16 bytes; una IPv4 va en los 4 primeros.
    pub dirs: [[u8; 16]; p::MAX_DIRECCIONES],
    /// Los segundos que valen, y de dónde salen (`DE_RED`, `DE_CACHE`, `DE_HOSTS`).
    pub ttl: u32,
    pub origen: u32,
}

impl Direcciones {
    /// La primera, como IPv4.
    pub fn v4(&self) -> [u8; 4] {
        [self.dirs[0][0], self.dirs[0][1], self.dirs[0][2], self.dirs[0][3]]
    }
}

/// Las direcciones de `nombre` (`familia`: `IPV4` para A, `IPV6` para AAAA), por el resolvedor del servicio. `servidor`:
/// a qué servidor de nombres preguntar (IPv4) y en qué puerto (0: el 53), o `[0; 4]` para el del sistema. Espera como
/// mucho `plazo_us` (el servicio reintenta él solo: tarda hasta 7 s en dar un nombre por perdido).
pub fn resolver(servicio: u32, nombre: &[u8], familia: u32, servidor: ([u8; 4], u16), plazo_us: u64) -> Result<Direcciones, i32> {
    if nombre.len() > p::MAX_NOMBRE {
        return Err(p::MAL_FORMADO);
    }
    let mut m = [0u8; 16 + p::MAX_NOMBRE];
    for (i, v) in [p::RESOLVER, familia | (servidor.1 as u32) << 16, u32::from_le_bytes(servidor.0), nombre.len() as u32].into_iter().enumerate() {
        m[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    m[16..16 + nombre.len()].copy_from_slice(nombre);
    let (lee, escribe) = crate::canal_crear();
    let fin = crate::ahora_us() + plazo_us;
    let copia = crate::llave_duplicar(escribe, crate::ESCRIBIR | crate::TRANSFERIR, 0);
    let mut r = Err(p::LIMITE);
    if copia > 0 {
        // Con la cola del servicio llena, se reintenta dentro del plazo.
        let mandado = loop {
            match crate::canal_escribir(servicio, &m[..16 + nombre.len()], &[copia as u32]) {
                0 => break true,
                -8 if crate::ahora_us() < fin => crate::dormir_us(2_000),
                _ => break false,
            }
        };
        if !mandado {
            crate::cerrar(copia as u32);
        } else {
            let mut b = [0u8; 20 + 16 * p::MAX_DIRECCIONES];
            let mut ll = [0u32; 8];
            let (n, k, _) = crate::canal_leer_plazo(lee, &mut b, &mut ll, fin.saturating_sub(crate::ahora_us()).max(1));
            let k = if n >= 0 { k.min(ll.len()) } else { 0 };
            ll[..k].iter().for_each(|&l| {
                crate::cerrar(l);
            });
            let w = |i: usize| u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
            r = if n < 20 || w(0) != p::R_RESOLVER {
                Err(p::SIN_RESPUESTA)
            } else if w(1) as i32 != p::BIEN {
                Err(w(1) as i32)
            } else {
                let cuantas = (w(2) as usize).min(p::MAX_DIRECCIONES).min((n as usize - 20) / 16);
                let mut d = Direcciones { n: cuantas, dirs: [[0; 16]; p::MAX_DIRECCIONES], ttl: w(3), origen: w(4) };
                for i in 0..cuantas {
                    d.dirs[i].copy_from_slice(&b[20 + 16 * i..36 + 16 * i]);
                }
                if cuantas == 0 { Err(p::SIN_NOMBRE) } else { Ok(d) }
            };
        }
    }
    crate::cerrar(lee);
    crate::cerrar(escribe);
    r
}

/// Una consulta de nombre pedida sin esperar (`resolver_empezar`).
pub struct Resolucion {
    lee: u32,
    escribe: u32,
}

/// Empieza a resolver `nombre` (como `resolver`) sin esperar: `Resolucion::mirar` da la respuesta cuando llegue.
pub fn resolver_empezar(servicio: u32, nombre: &[u8], familia: u32, servidor: ([u8; 4], u16)) -> Result<Resolucion, i32> {
    if nombre.len() > p::MAX_NOMBRE {
        return Err(p::MAL_FORMADO);
    }
    let mut m = [0u8; 16 + p::MAX_NOMBRE];
    for (i, v) in [p::RESOLVER, familia | (servidor.1 as u32) << 16, u32::from_le_bytes(servidor.0), nombre.len() as u32].into_iter().enumerate() {
        m[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
    }
    m[16..16 + nombre.len()].copy_from_slice(nombre);
    let (lee, escribe) = crate::canal_crear();
    let copia = crate::llave_duplicar(escribe, crate::ESCRIBIR | crate::TRANSFERIR, 0);
    let fin = crate::ahora_us() + 50_000;
    let mandado = copia > 0
        && loop {
            match crate::canal_escribir(servicio, &m[..16 + nombre.len()], &[copia as u32]) {
                0 => break true,
                -8 if crate::ahora_us() < fin => crate::dormir_us(2_000),
                _ => break false,
            }
        };
    if !mandado {
        if copia > 0 {
            crate::cerrar(copia as u32);
        }
        crate::cerrar(lee);
        crate::cerrar(escribe);
        return Err(p::LIMITE);
    }
    Ok(Resolucion { lee, escribe })
}

impl Resolucion {
    /// La respuesta, si ya llegó (entonces la consulta queda acabada: no se vuelve a mirar); `None` si aún no.
    pub fn mirar(&mut self) -> Option<Result<Direcciones, i32>> {
        if self.lee == 0 {
            return Some(Err(p::NO_EXISTE));
        }
        let mut b = [0u8; 20 + 16 * p::MAX_DIRECCIONES];
        let mut ll = [0u32; 8];
        let (n, k, _) = crate::canal_leer_plazo(self.lee, &mut b, &mut ll, 1);
        let k = if n >= 0 { k.min(ll.len()) } else { 0 };
        ll[..k].iter().for_each(|&l| {
            crate::cerrar(l);
        });
        if n < 20 {
            return None;
        }
        let w = |i: usize| u32::from_le_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
        let r = if w(0) != p::R_RESOLVER {
            Err(p::SIN_RESPUESTA)
        } else if w(1) as i32 != p::BIEN {
            Err(w(1) as i32)
        } else {
            let cuantas = (w(2) as usize).min(p::MAX_DIRECCIONES).min((n as usize - 20) / 16);
            let mut d = Direcciones { n: cuantas, dirs: [[0; 16]; p::MAX_DIRECCIONES], ttl: w(3), origen: w(4) };
            for i in 0..cuantas {
                d.dirs[i].copy_from_slice(&b[20 + 16 * i..36 + 16 * i]);
            }
            if cuantas == 0 { Err(p::SIN_NOMBRE) } else { Ok(d) }
        };
        self.soltar();
        Some(r)
    }

    /// Suelta la consulta (si la respuesta llega después, se pierde).
    pub fn soltar(&mut self) {
        if self.lee != 0 {
            crate::cerrar(self.lee);
            crate::cerrar(self.escribe);
            self.lee = 0;
        }
    }
}

/// Un zócalo UDP: datagramas a cualquiera y, de vuelta, de quienes se les envió (o de cualquiera, si se abrió con el
/// permiso de escuchar).
pub struct Udp {
    c: Conexion,
}

impl Udp {
    /// Abre un zócalo UDP por el canal `servicio`. Con `puerto` 0, el que toque; uno elegido solo vale por el canal
    /// "red-escuchar" (el permiso de escuchar). La memoria se proyecta en `va`.
    pub fn abrir(servicio: u32, puerto: u16, va: usize, tam: usize, plazo_us: u64) -> Result<Udp, i32> {
        Conexion::pedir(servicio, &[p::UDP, puerto as u32, tam as u32], va, tam, plazo_us).map(|c| Udp { c })
    }

    /// Lo mismo sin esperar: `Pendiente::mirar` da la `Conexion`, y `Udp::de_conexion` el zócalo.
    pub fn empezar(servicio: u32, puerto: u16, va: usize, tam: usize) -> Result<Pendiente, i32> {
        Conexion::empezar_pedir(servicio, &[p::UDP, puerto as u32, tam as u32], va, tam)
    }

    /// El zócalo de una conexión que llegó de `Udp::empezar`.
    pub fn de_conexion(c: Conexion) -> Udp {
        Udp { c }
    }

    /// Bytes esperando en el anillo de recepción (con sus cabeceras) y sitio en el de envío.
    pub fn pendiente(&self) -> (usize, usize) {
        self.c.pendiente()
    }

    /// El puerto de este lado.
    pub fn puerto(&self) -> u16 {
        self.c.puerto()
    }

    /// `ABIERTA` o un error (`MAL_FORMADO`: el servicio encontró una cabecera imposible en el anillo).
    pub fn fase(&self) -> i32 {
        self.c.fase()
    }

    /// Datagramas que llegaron y se perdieron (anillo lleno, o de alguien a quien no se había enviado).
    pub fn perdidos(&self) -> u32 {
        self.c.palabra(p::PERDIDOS)
    }

    /// Copia `datos` al anillo que empieza en `base`, desde la posición `pos` (da la vuelta).
    fn poner(&self, base: usize, pos: u32, datos: &[u8]) {
        let desde = pos as usize & (self.c.tam - 1);
        let primero = datos.len().min(self.c.tam - desde);
        // Memoria compartida con el servicio: copia de bytes, sin más.
        unsafe {
            core::ptr::copy_nonoverlapping(datos.as_ptr(), (base + desde) as *mut u8, primero);
            core::ptr::copy_nonoverlapping(datos[primero..].as_ptr(), base as *mut u8, datos.len() - primero);
        }
    }

    fn sacar(&self, base: usize, pos: u32, sal: &mut [u8]) {
        let desde = pos as usize & (self.c.tam - 1);
        let primero = sal.len().min(self.c.tam - desde);
        unsafe {
            core::ptr::copy_nonoverlapping((base + desde) as *const u8, sal.as_mut_ptr(), primero);
            core::ptr::copy_nonoverlapping(base as *const u8, sal[primero..].as_mut_ptr(), sal.len() - primero);
        }
    }

    /// Pone un datagrama para `ip` (16 bytes; IPv4 en los 4 primeros) de la `familia` y `puerto`. Falso si no cabe en
    /// el anillo de envío (o mide más de `MAX_DATAGRAMA`). No espera.
    pub fn enviar(&self, datos: &[u8], ip: [u8; 16], familia: u32, puerto: u16) -> bool {
        let cabeza = self.c.palabra(p::ENV_CABEZA);
        let ocupado = cabeza.wrapping_sub(self.c.palabra(p::ENV_COLA)) as usize;
        if datos.len() > p::MAX_DATAGRAMA || self.c.tam.saturating_sub(ocupado) < p::UDP_CABECERA + datos.len() {
            return false;
        }
        let mut cab = [0u8; p::UDP_CABECERA];
        cab[..2].copy_from_slice(&(datos.len() as u16).to_le_bytes());
        cab[2..4].copy_from_slice(&puerto.to_le_bytes());
        cab[4] = familia as u8;
        cab[8..24].copy_from_slice(&ip);
        let base = self.c.va + p::DATOS;
        self.poner(base, cabeza, &cab);
        self.poner(base, cabeza.wrapping_add(p::UDP_CABECERA as u32), datos);
        fence(Ordering::SeqCst);
        self.c.poner(p::ENV_CABEZA, cabeza.wrapping_add((p::UDP_CABECERA + datos.len()) as u32));
        self.c.avisar();
        true
    }

    /// Lo mismo, a una dirección IPv4.
    pub fn enviar4(&self, datos: &[u8], ip: [u8; 4], puerto: u16) -> bool {
        let mut d = [0u8; 16];
        d[..4].copy_from_slice(&ip);
        self.enviar(datos, d, p::IPV4, puerto)
    }

    /// Saca el siguiente datagrama que llegó, si lo hay: cuánto medía (si no cabe en `buf`, lo que sobra se tira), y de
    /// quién viene: dirección, familia y puerto. No espera.
    pub fn recibir(&self, buf: &mut [u8]) -> Option<(usize, [u8; 16], u32, u16)> {
        let cola = self.c.palabra(p::REC_COLA);
        let hay = (self.c.palabra(p::REC_CABEZA).wrapping_sub(cola) as usize).min(self.c.tam);
        if hay < p::UDP_CABECERA {
            return None;
        }
        fence(Ordering::SeqCst);
        let base = self.c.va + p::DATOS + self.c.tam;
        let mut cab = [0u8; p::UDP_CABECERA];
        self.sacar(base, cola, &mut cab);
        let largo = (u16::from_le_bytes([cab[0], cab[1]]) as usize).min(hay - p::UDP_CABECERA);
        let n = largo.min(buf.len());
        self.sacar(base, cola.wrapping_add(p::UDP_CABECERA as u32), &mut buf[..n]);
        fence(Ordering::SeqCst);
        self.c.poner(p::REC_COLA, cola.wrapping_add((p::UDP_CABECERA + largo) as u32));
        let mut ip = [0u8; 16];
        ip.copy_from_slice(&cab[8..24]);
        Some((largo, ip, cab[4] as u32, u16::from_le_bytes([cab[2], cab[3]])))
    }

    /// Duerme hasta que llegue algo (o `plazo_us`). Si ya hay algo que leer, vuelve.
    pub fn esperar(&self, plazo_us: u64) {
        self.c.esperar(true, false, plazo_us);
    }

    /// Cierra: el servicio manda lo que quede y suelta el zócalo.
    pub fn cerrar(self) {
        self.c.cerrar();
    }
}

/// Esperar a que alguien entre por un puerto TCP (`ESCUCHAR`): solo por el canal "red-escuchar" (el permiso de
/// escuchar). Atiende a uno: para atender a varios a la vez, varios en el mismo puerto.
pub struct Oido {
    c: Option<Conexion>,
}

impl Oido {
    /// Empieza a escuchar en `puerto`. La memoria de la conexión que entre se proyectará en `va`.
    pub fn escuchar(servicio: u32, puerto: u16, va: usize, tam: usize, plazo_us: u64) -> Result<Oido, i32> {
        let (lee, escribe) = crate::canal_crear();
        let mut c = Conexion { servicio, id: 0, va, tam, memoria: 0, lee, escribe };
        let fin = crate::ahora_us() + plazo_us;
        c.mandar(&[p::ESCUCHAR, puerto as u32, tam as u32], fin)?;
        let mut b = [0u32; 4];
        let mut ll = [0u32; 8];
        loop {
            let ahora = crate::ahora_us();
            if ahora >= fin {
                c.soltar();
                return Err(p::RECHAZADA);
            }
            let bytes = unsafe { core::slice::from_raw_parts_mut(b.as_mut_ptr() as *mut u8, 16) };
            let (n, k, _) = crate::canal_leer_plazo(lee, bytes, &mut ll, fin - ahora);
            de_le(&mut b);
            let k = if n >= 0 { k.min(ll.len()) } else { 0 };
            ll[..k].iter().for_each(|&l| {
                crate::cerrar(l);
            });
            if n >= 12 && b[0] == p::R_ESCUCHAR {
                if b[2] as i32 != p::BIEN {
                    c.soltar();
                    return Err(b[2] as i32);
                }
                c.id = b[1];
                return Ok(Oido { c: Some(c) });
            }
        }
    }

    /// Espera como mucho `plazo_us` a que alguien entre. `Ok(None)` si aún no ha entrado nadie (se puede volver a
    /// llamar); con la conexión, este oído ya no sirve (se pide otro para el siguiente).
    pub fn aceptar(&mut self, plazo_us: u64) -> Result<Option<Conexion>, i32> {
        let Some(c) = self.c.as_ref() else { return Err(p::NO_EXISTE) };
        let fin = crate::ahora_us() + plazo_us;
        let (lee, id, va, tam) = (c.lee, c.id, c.va, c.tam);
        let mut b = [0u32; 4];
        let mut ll = [0u32; 8];
        let mut primera = true;
        loop {
            let ahora = crate::ahora_us();
            // Con plazo 0 (o ya vencido) se mira una vez sin esperar.
            if ahora >= fin && !primera {
                return Ok(None);
            }
            primera = false;
            let bytes = unsafe { core::slice::from_raw_parts_mut(b.as_mut_ptr() as *mut u8, 16) };
            let (n, k, _) = crate::canal_leer_plazo(lee, bytes, &mut ll, fin.saturating_sub(ahora).max(1));
            de_le(&mut b);
            let k = if n >= 0 { k.min(ll.len()) } else { 0 };
            if n >= 12 && b[0] == p::R_CONECTAR && b[1] == id {
                let c = self.c.take().unwrap();
                if b[2] as i32 != p::BIEN || k != 1 {
                    ll[..k].iter().for_each(|&l| {
                        crate::cerrar(l);
                    });
                    c.soltar();
                    return Err(if b[2] as i32 != p::BIEN { b[2] as i32 } else { p::MAL_FORMADO });
                }
                let memoria = ll[0];
                if crate::memoria_tamano(memoria) < (p::DATOS + 2 * tam) as i64 || crate::memoria_proyectar(memoria, va, true) != 0 {
                    crate::cerrar(memoria);
                    crate::canal_escribir(c.servicio, palabras(&[p::CERRAR, id], &mut [0; 32]), &[]);
                    c.soltar();
                    return Err(p::MAL_FORMADO);
                }
                return Ok(Some(Conexion { memoria, ..c }));
            }
            ll[..k].iter().for_each(|&l| {
                crate::cerrar(l);
            });
        }
    }

    /// Deja de escuchar.
    pub fn cerrar(mut self) {
        if let Some(c) = self.c.take() {
            c.cerrar();
        }
    }
}
