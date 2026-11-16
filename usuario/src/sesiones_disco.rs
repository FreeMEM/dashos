//! Lo que `sesiones` guarda en el disco con clientes suyos del servicio de archivos aparte del de siempre: el registro
//! común (`Estado:Registros/`, `dashos::registro`) y los permisos concedidos (`Home:Prefs/Permisos/` de cada persona y
//! `Estado:Prefs/Permisos/` para los programas del sistema, `dashos::permisos_texto`).
//!
//! Cada cliente nace con un alta que `sesiones` manda sin marca (como a lo que lanza), con su usuario (el de `sesiones`)
//! y una raíz con `HUECO_SUYO`: el cajón se crea si falta, suyo, y si ya está y no es suyo no se le da. Así nadie
//! cambia el cajón de sitio y pone otro con lo que quiera. El del registro vive siempre; el de los permisos de una
//! persona, lo que dura leer o escribir (si se quedara, su volumen no se podría cerrar al salir).

use dashos::permisos_texto as pt;
use dashos::protocolo::archivos as a;
use dashos::registro as r;

/// Lo que se espera al servicio de archivos.
const PLAZO_US: u64 = 5_000_000;

/// Un cliente del servicio de archivos con su propia marca.
pub struct Cliente {
    escribir: u32,
    lectura: u32,
    va: usize,
    tam: usize,
    marca: u64,
}

impl Cliente {
    /// Da de alta (por `altas`, la llave sin marca del servicio) a `marca`, del usuario `uid`, con la raíz 0 en `raiz0`
    /// del disco (tiene que existir) y, en el hueco 1, el cajón `cam` del volumen `vol` (vacío: el disco), creado si
    /// falta; y abre su sesión, con la ventana en `va`.
    pub fn abrir(altas: u32, marca: u64, uid: u32, raiz0: &[u8], vol: &[u8], cam: &[u8], va: usize) -> Option<Cliente> {
        let mut m = [0u8; 256];
        if raiz0.len() > 64 || vol.len() > 16 || cam.len() > 100 {
            return None;
        }
        m[0..4].copy_from_slice(&a::ALTA.to_le_bytes());
        m[8..16].copy_from_slice(&marca.to_le_bytes());
        m[16..20].copy_from_slice(&uid.to_le_bytes());
        m[20..24].copy_from_slice(&uid.to_le_bytes());
        m[24..28].copy_from_slice(&(raiz0.len() as u32).to_le_bytes());
        let mut o = a::CABECERA;
        m[o..o + raiz0.len()].copy_from_slice(raiz0);
        o += raiz0.len();
        m[o..o + 4].copy_from_slice(&[1 | a::HUECO_SUYO, 6, vol.len() as u8, cam.len() as u8]);
        o += 4;
        m[o..o + vol.len()].copy_from_slice(vol);
        o += vol.len();
        m[o..o + cam.len()].copy_from_slice(cam);
        o += cam.len();
        if dashos::canal_escribir(altas, &m[..o], &[]) != 0 {
            return None;
        }
        let escribir = u32::try_from(dashos::llave_duplicar(altas, dashos::ESCRIBIR | dashos::TRANSFERIR, marca)).ok()?;
        let (lectura, suyo) = dashos::canal_crear();
        let mut p = [0u8; a::CABECERA];
        p[0..4].copy_from_slice(&a::SESION.to_le_bytes());
        let mut rr = [0u8; 32];
        let mut ll = [0u32; 1];
        let bien = dashos::canal_escribir(escribir, &p, &[suyo]) == 0 && {
            let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut rr, &mut ll, PLAZO_US);
            n == 32 && k == 1 && i32::from_le_bytes(rr[0..4].try_into().unwrap()) == a::BIEN && dashos::memoria_proyectar(ll[0], va, true) == 0
        };
        let c = Cliente { escribir, lectura, va, tam: (u64::from_le_bytes(rr[8..16].try_into().unwrap()) as usize).clamp(4096, a::VENTANA_MAX), marca };
        // Y la raíz 1 tiene que estar, y ser suya: si el cajón no se pudo crear o es de otro, el alta no la dio.
        if !bien || !c.es_suyo(1) {
            c.cerrar(altas);
            return None;
        }
        Some(c)
    }

    /// Una orden: (estado, asa, a, b) de la respuesta.
    pub fn orden(&self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> (i32, u32, u64, u64) {
        let mut p = [0u8; 256];
        let largo = nombre.len().min(256 - a::CABECERA);
        p[0..4].copy_from_slice(&orden.to_le_bytes());
        p[4..8].copy_from_slice(&asa.to_le_bytes());
        p[8..16].copy_from_slice(&x.to_le_bytes());
        p[16..24].copy_from_slice(&y.to_le_bytes());
        p[24..28].copy_from_slice(&(largo as u32).to_le_bytes());
        p[32..32 + largo].copy_from_slice(&nombre[..largo]);
        if dashos::canal_escribir(self.escribir, &p[..32 + largo], &[]) != 0 {
            return (a::ERROR_DEL_DISCO, 0, 0, 0);
        }
        let mut rr = [0u8; 32];
        if dashos::canal_leer_plazo(self.lectura, &mut rr, &mut [], PLAZO_US).0 != 32 {
            return (a::ERROR_DEL_DISCO, 0, 0, 0);
        }
        let u = |i: usize| u64::from_le_bytes(rr[i..i + 8].try_into().unwrap());
        (i32::from_le_bytes(rr[0..4].try_into().unwrap()), u32::from_le_bytes(rr[4..8].try_into().unwrap()), u(8), u(16))
    }

    pub fn ventana(&self) -> &mut [u8] {
        // SAFETY: la ventana del servicio, proyectada en `abrir` con ese tamaño; solo la usa el hilo principal de
        // `sesiones`, y una sola vez a la vez (cada cliente tiene su dirección).
        unsafe { core::slice::from_raw_parts_mut(self.va as *mut u8, self.tam) }
    }

    /// Si lo de `asa` es de este cliente (su usuario es el dueño): lo que no, no se toma por suyo.
    pub fn es_suyo(&self, asa: u32) -> bool {
        let (e, _, dueno, _) = self.orden(a::DUENO, asa, 0, 0, b"");
        // DUENO da el dueño en a: tiene que ser el usuario con que se dio de alta este cliente, el de `sesiones`.
        e == a::BIEN && dueno == SESIONES_UID as u64
    }

    /// Fuera: la baja en el servicio (suelta sus asas y raíces) y sus llaves.
    pub fn cerrar(self, altas: u32) {
        let mut b = [0u8; a::CABECERA];
        b[0..4].copy_from_slice(&a::BAJA.to_le_bytes());
        b[8..16].copy_from_slice(&self.marca.to_le_bytes());
        dashos::canal_escribir(altas, &b, &[]);
        dashos::memoria_quitar(self.va);
        dashos::cerrar(self.escribir);
        dashos::cerrar(self.lectura);
    }
}

/// El usuario de `sesiones` (S6b: el 1, como lo da `inicio`): el dueño de lo que escribe.
pub const SESIONES_UID: u32 = 1;

/// Un texto con formato en un búfer fijo (`sesiones` no tiene montón). Lo que no cabe se pierde.
pub struct Texto<const N: usize> {
    pub b: [u8; N],
    pub n: usize,
}

impl<const N: usize> Texto<N> {
    pub fn nuevo() -> Self {
        Self { b: [0; N], n: 0 }
    }

    pub fn de(args: core::fmt::Arguments) -> Self {
        let mut t = Self::nuevo();
        let _ = core::fmt::Write::write_fmt(&mut t, args);
        t
    }

    pub fn bytes(&self) -> &[u8] {
        &self.b[..self.n]
    }
}

impl<const N: usize> core::fmt::Write for Texto<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let k = s.len().min(N - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&s.as_bytes()[..k]);
        self.n += k;
        Ok(())
    }
}

/// El nombre con que sale el usuario `uid` en el registro: el de su cuenta, `sistema` si no es una persona, o su número.
pub fn usuario(uid: u32, cuentas: &[(u32, &str)], primera_persona: u32) -> Texto<16> {
    match cuentas.iter().find(|c| c.0 == uid) {
        Some(c) => Texto::de(format_args!("{}", c.1)),
        None if uid < primera_persona => Texto::de(format_args!("{}", r::DEL_SISTEMA)),
        None => Texto::de(format_args!("{}", uid)),
    }
}

/// La marca del cliente del registro (ningún programa tiene una así: las suyas son su número).
const MARCA_REGISTRO: u64 = 0x7265_6700_0000_0001;
/// Dónde ve su ventana el cliente del registro, y donde se copia lo que se da a leer.
const VA_REGISTRO: usize = 0x50_0000_0000;
const VA_LEER: usize = 0x54_0000_0000;

/// El registro común en el disco: un cliente propio con su cajón (`Estado/Registros`, de `sesiones`, 0700) y, por cada
/// fichero, su asa, lo que mide y cuántas veces ha rotado.
pub struct Registro {
    c: Option<Cliente>,
    /// La llave sin marca del servicio de archivos, para volver a dar de alta al cliente.
    altas: u32,
    f: [(u32, u64, u32); 3],
    /// Líneas que no llegaron al disco (sin cliente o con un fallo), para decirlo.
    pub perdidas: u64,
}

impl Registro {
    pub const fn sin_disco() -> Registro {
        Registro { c: None, altas: 0, f: [(0, 0, 0); 3], perdidas: 0 }
    }

    /// Abre (o crea) el cajón y sus ficheros. `altas`: la llave sin marca del servicio de archivos.
    pub fn abrir(&mut self, altas: u32) -> Result<(), i32> {
        self.altas = altas;
        let raiz0 = r::CAJON.rsplit_once('/').map_or("", |x| x.0);
        let c = Cliente::abrir(altas, MARCA_REGISTRO, SESIONES_UID, raiz0.as_bytes(), b"", r::CAJON.as_bytes(), VA_REGISTRO).ok_or(-1)?;
        // Solo de `sesiones`: ni el dueño del resto del disco lo lista (cada uno lee lo suyo por `sesiones`).
        let (e, ..) = c.orden(a::PROTEGER, 1, 0, 0o700, b"");
        if e != a::BIEN {
            c.cerrar(altas);
            return Err(e);
        }
        self.c = Some(c);
        for i in 0..3 {
            self.abrir_fichero(i)?;
        }
        Ok(())
    }

    /// Otra vez de cero: volver a una instantánea del disco suelta las asas de todos y las raíces que no son la de un
    /// volumen (la del cajón del registro, también): sin esto, no se escribía ni se leía nada más. Lo que se apuntó
    /// después de la instantánea se pierde con ella (es lo que hace volver).
    fn reabrir(&mut self) -> bool {
        if let Some(c) = self.c.take() {
            c.cerrar(self.altas);
        }
        self.altas != 0 && self.abrir(self.altas).is_ok()
    }

    fn abrir_fichero(&mut self, i: usize) -> Result<(), i32> {
        let c = self.c.as_ref().ok_or(-1)?;
        let nombre = r::FICHEROS[i].as_bytes();
        let (e, asa, tam, _) = c.orden(a::BUSCAR, 1, 0, 0, nombre);
        let (asa, tam) = if e == a::BIEN && c.es_suyo(asa) {
            (asa, tam)
        } else {
            if e == a::BIEN {
                // No es de `sesiones` (no debería pasar: el cajón sí lo es): fuera, y uno nuevo.
                c.orden(a::CERRAR, asa, 0, 0, b"");
                c.orden(a::BORRAR, 1, 0, 0, nombre);
            }
            let (e, asa, ..) = c.orden(a::CREAR, 1, a::FICHERO as u64, 0, nombre);
            if e != a::BIEN {
                return Err(e);
            }
            (asa, 0)
        };
        self.f[i].0 = asa;
        self.f[i].1 = tam;
        Ok(())
    }

    /// Añade una línea ya hecha (`dashos::registro::linea`) al fichero `i`: al final, y si pasa del tope, el de ahora
    /// pasa a `.1` y se empieza otro.
    pub fn anadir(&mut self, i: usize, linea: &[u8]) {
        if i >= 3 || linea.is_empty() {
            return;
        }
        for intento in 0..2 {
            if self.c.is_none() {
                self.perdidas += 1;
                return;
            }
            if r::rota(self.f[i].1, linea.len()) {
                self.rotar(i);
            }
            let c = self.c.as_ref().unwrap();
            let (asa, tam, _) = self.f[i];
            let v = c.ventana();
            v[..linea.len()].copy_from_slice(linea);
            let (e, _, escrito, _) = c.orden(a::ESCRIBIR, asa, tam, linea.len() as u64, b"");
            if e == a::BIEN && escrito == linea.len() as u64 {
                self.f[i].1 += escrito;
                return;
            }
            // Se perdió la asa (volvieron a una instantánea del disco, que suelta las de todos): se abre otra vez, y si
            // no se puede (también se perdió la raíz del cajón), el cliente entero.
            if intento == 0 && (self.abrir_fichero(i).is_ok() || self.reabrir()) {
                continue;
            }
            self.perdidas += 1;
            return;
        }
    }

    fn rotar(&mut self, i: usize) {
        let Some(c) = self.c.as_ref() else { return };
        let nombre = r::FICHEROS[i].as_bytes();
        let mut viejo = [0u8; 32];
        let n = nombre.len();
        viejo[..n].copy_from_slice(nombre);
        viejo[n..n + 2].copy_from_slice(b".1");
        c.orden(a::CERRAR, self.f[i].0, 0, 0, b"");
        c.orden(a::BORRAR, 1, 0, 0, &viejo[..n + 2]);
        let mut doble = [0u8; 64];
        doble[..n].copy_from_slice(nombre);
        doble[n + 1..2 * n + 1].copy_from_slice(nombre);
        doble[2 * n + 1..2 * n + 3].copy_from_slice(b".1");
        c.orden(a::RENOMBRAR, 1, 0, 1, &doble[..2 * n + 3]);
        self.f[i].2 = self.f[i].2.wrapping_add(1);
        let (e, asa, ..) = c.orden(a::CREAR, 1, a::FICHERO as u64, 0, nombre);
        self.f[i].0 = if e == a::BIEN { asa } else { 0 };
        self.f[i].1 = 0;
    }

    /// Lo que puede leer `lector` (el nombre de su cuenta; vacío si no es una persona; con `todo`, lo de todos) del
    /// fichero `i`: con `parte` 0, el de ahora, y con 1, el `.1`, desde el byte `desde`. Devuelve (estado, hasta qué
    /// byte del fichero llegó, cuántas veces ha rotado, una llave de memoria solo para leer con las líneas que ve y
    /// cuánto miden). Si `desde` pasa del final (rotó entre medias), desde el principio.
    pub fn leer(&mut self, i: usize, parte: u32, desde: u64, lector: &str, todo: bool) -> (i32, u64, u32, u32, usize) {
        let r_ = self.leer_una_vez(i, parte, desde, lector, todo);
        // Se perdió la asa (volvieron a una instantánea del disco): se abre otra vez y se lee de nuevo.
        if r_.0 == a::ASA_MALA && ((parte == 0 && self.abrir_fichero(i).is_ok()) || self.reabrir()) {
            let r2 = self.leer_una_vez(i, parte, desde, lector, todo);
            if r2.0 != a::ASA_MALA || !self.reabrir() {
                return r2;
            }
            return self.leer_una_vez(i, parte, desde, lector, todo);
        }
        r_
    }

    fn leer_una_vez(&mut self, i: usize, parte: u32, desde: u64, lector: &str, todo: bool) -> (i32, u64, u32, u32, usize) {
        let Some(c) = self.c.as_ref() else { return (a::NO_EXISTE, 0, 0, 0, 0) };
        if i >= 3 || parte > 1 {
            return (a::MAL_FORMADO, 0, 0, 0, 0);
        }
        let generacion = self.f[i].2;
        let (asa, tam, propia) = if parte == 0 {
            (self.f[i].0, self.f[i].1, false)
        } else {
            let mut viejo = [0u8; 32];
            let n = r::FICHEROS[i].len();
            viejo[..n].copy_from_slice(r::FICHEROS[i].as_bytes());
            viejo[n..n + 2].copy_from_slice(b".1");
            let (e, asa, tam, _) = c.orden(a::BUSCAR, 1, 0, 0, &viejo[..n + 2]);
            if e != a::BIEN {
                return (a::BIEN, 0, generacion, 0, 0);
            }
            (asa, tam, true)
        };
        let desde = if desde > tam { 0 } else { desde };
        let cuanto = (tam - desde) as usize;
        let mut r_ = (a::BIEN, tam, generacion, 0, 0);
        if cuanto > 0 {
            r_ = match leer_a_memoria(c, asa, desde, cuanto) {
                Ok(m) => {
                    // SAFETY: el objeto de memoria recién proyectado en VA_LEER, de `cuanto` bytes como poco.
                    let v = unsafe { core::slice::from_raw_parts_mut(VA_LEER as *mut u8, cuanto) };
                    let largo = quedarse_con(v, lector, todo);
                    dashos::memoria_quitar(VA_LEER);
                    let solo_leer = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
                    dashos::cerrar(m);
                    if solo_leer > 0 { (a::BIEN, tam, generacion, solo_leer as u32, largo) } else { (a::SIN_SITIO, 0, 0, 0, 0) }
                }
                Err(e) => (e, 0, 0, 0, 0),
            };
        }
        if propia {
            c.orden(a::CERRAR, asa, 0, 0, b"");
        }
        r_
    }
}

/// Lee `cuanto` bytes de `asa` desde `desde` a un objeto de memoria nuevo, proyectado en `VA_LEER`.
fn leer_a_memoria(c: &Cliente, asa: u32, desde: u64, cuanto: usize) -> Result<u32, i32> {
    let m = dashos::memoria_crear(cuanto);
    if (m as i32) <= 0 || dashos::memoria_proyectar(m, VA_LEER, true) != 0 {
        if (m as i32) > 0 {
            dashos::cerrar(m);
        }
        return Err(a::SIN_SITIO);
    }
    let mut hecho = 0;
    while hecho < cuanto {
        let (e, _, n, _) = c.orden(a::LEER, asa, desde + hecho as u64, (cuanto - hecho).min(c.tam) as u64, b"");
        if e != a::BIEN || n == 0 {
            dashos::memoria_quitar(VA_LEER);
            dashos::cerrar(m);
            return Err(if e != a::BIEN { e } else { a::ERROR_DEL_DISCO });
        }
        let n = (n as usize).min(cuanto - hecho);
        // SAFETY: la ventana tiene `n` bytes leídos, y `VA_LEER` `cuanto` proyectados para escribir.
        unsafe { core::ptr::copy_nonoverlapping(c.va as *const u8, (VA_LEER + hecho) as *mut u8, n) };
        hecho += n;
    }
    Ok(m)
}

/// Deja al principio de `v` solo las líneas enteras que ve `lector`, una tras otra; devuelve lo que miden.
pub fn quedarse_con(v: &mut [u8], lector: &str, todo: bool) -> usize {
    let mut leo = 0;
    let mut escribo = 0;
    while leo < v.len() {
        let Some(fin) = v[leo..].iter().position(|&c| c == b'\n') else { break };
        let linea = leo..leo + fin + 1;
        let ve = core::str::from_utf8(&v[linea.clone()]).is_ok_and(|l| r::ve(lector, todo, l));
        if ve {
            v.copy_within(linea.clone(), escribo);
            escribo += linea.len();
        }
        leo = linea.end;
    }
    escribo
}

/// La marca del cliente de los permisos (uno a la vez, y solo mientras lee o escribe).
const MARCA_PERMISOS: u64 = 0x7065_7200_0000_0001;
const VA_PERMISOS: usize = 0x58_0000_0000;

/// Por qué no se pudo leer o escribir lo concedido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallo {
    /// El cajón no se pudo crear o no es de `sesiones` (no se le dio la raíz).
    Cajon,
    /// El fichero no es de `sesiones`: se ignora.
    Ajeno,
    /// El servicio dijo que no.
    Disco(i32),
}

/// Lee lo concedido del cajón `cam` del volumen `vol` (el de una persona, o vacío: el disco) y llama a `cada` con cada
/// línea que vale. Sin fichero, nada y bien. Un fichero que no es de `sesiones` no se lee.
pub fn leer_permisos(altas: u32, vol: &[u8], cam: &[u8], mut cada: impl FnMut(&[u8], &[u8], Option<u32>)) -> Result<usize, Fallo> {
    let c = Cliente::abrir(altas, MARCA_PERMISOS, SESIONES_UID, b"Estado", vol, cam, VA_PERMISOS).ok_or(Fallo::Cajon)?;
    let r_ = (|| {
        let (e, asa, tam, _) = c.orden(a::BUSCAR, 1, 0, 0, pt::FICHERO.as_bytes());
        if e == a::NO_EXISTE {
            return Ok(0);
        }
        if e != a::BIEN {
            return Err(Fallo::Disco(e));
        }
        let suyo = c.es_suyo(asa);
        let leido = if suyo { c.orden(a::LEER, asa, 0, (tam as usize).min(pt::MAX_FICHERO).min(c.tam) as u64, b"") } else { (a::BIEN, 0, 0, 0) };
        c.orden(a::CERRAR, asa, 0, 0, b"");
        if !suyo {
            return Err(Fallo::Ajeno);
        }
        if leido.0 != a::BIEN {
            return Err(Fallo::Disco(leido.0));
        }
        let texto = &c.ventana()[..leido.2 as usize];
        let mut n = 0;
        for (p, s, u) in pt::lineas(texto) {
            cada(p, s, u);
            n += 1;
        }
        Ok(n)
    })();
    c.cerrar(altas);
    r_
}

/// Escribe entero el fichero de lo concedido en el cajón `cam` del volumen `vol`: la cabecera y las líneas que da
/// `lineas` (programa, permiso y, en el del sistema, el usuario).
pub fn guardar_permisos<'x>(altas: u32, vol: &[u8], cam: &[u8], lineas: impl Iterator<Item = (&'x [u8], &'x [u8], Option<u32>)>) -> Result<usize, Fallo> {
    let c = Cliente::abrir(altas, MARCA_PERMISOS, SESIONES_UID, b"Estado", vol, cam, VA_PERMISOS).ok_or(Fallo::Cajon)?;
    let r_ = (|| {
        let v = c.ventana();
        let cab = pt::CABECERA.as_bytes();
        v[..cab.len()].copy_from_slice(cab);
        let mut o = cab.len();
        let mut n = 0;
        for (p, s, u) in lineas {
            let k = pt::escribir_linea(p, s, u, &mut v[o..pt::MAX_FICHERO.min(c.tam)]);
            o += k;
            n += (k > 0) as usize;
        }
        let nombre = pt::FICHERO.as_bytes();
        let (e, asa, ..) = c.orden(a::BUSCAR, 1, 0, 0, nombre);
        let asa = match e {
            a::BIEN if c.es_suyo(asa) => {
                let (e, ..) = c.orden(a::TAMANO, asa, 0, 0, b"");
                if e != a::BIEN {
                    c.orden(a::CERRAR, asa, 0, 0, b"");
                    return Err(Fallo::Disco(e));
                }
                asa
            }
            a::BIEN | a::NO_EXISTE => {
                if e == a::BIEN {
                    // Uno que no es de `sesiones` en su cajón: fuera (el cajón sí es suyo).
                    c.orden(a::CERRAR, asa, 0, 0, b"");
                    c.orden(a::BORRAR, 1, 0, 0, nombre);
                }
                let (e, asa, ..) = c.orden(a::CREAR, 1, a::FICHERO as u64, 0, nombre);
                if e != a::BIEN {
                    return Err(Fallo::Disco(e));
                }
                asa
            }
            e => return Err(Fallo::Disco(e)),
        };
        let (e, _, escrito, _) = c.orden(a::ESCRIBIR, asa, 0, o as u64, b"");
        c.orden(a::CERRAR, asa, 0, 0, b"");
        if e != a::BIEN || escrito != o as u64 {
            return Err(Fallo::Disco(if e != a::BIEN { e } else { a::LLENO }));
        }
        Ok(n)
    })();
    c.cerrar(altas);
    r_
}
