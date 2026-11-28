//! El adaptador de `SistemaArchivos` (dos.library, `caja/dos/src/fs.rs`) sobre el servicio `archivos` de DashOS
//! (`docs/referencia/servicios.md`, "Archivos"): cada método es una orden por el canal que da `inicio`, con la
//! respuesta por un canal propio y los datos por la ventana (64 KB de memoria del servicio que la caja proyecta).
//!
//! Todo va en little-endian, aunque la caja sea big-endian. Las raíces son las de la ficha de la caja en `inicio`
//! (`FICHAS`): la asa 0 es su raíz. dos solo manda nombres limpios desde asas que dio el servicio.

use alloc::string::String;
use alloc::vec::Vec;
use core::slice;

use caja_dos::fs::{Asa, Datos, Entrada, ErrorFs, InfoVolumen, Pagina, SistemaArchivos};
use dashos::protocolo::archivos as p;

/// Dónde se proyecta la ventana del servicio: por encima de 4 GB, fuera del alcance del 68k.
const VA_VENTANA: usize = 0x0000_0004_0000_0000;

#[derive(Clone, Copy, Default)]
struct Respuesta {
    estado: i32,
    asa: u32,
    a: u64,
    b: u64,
    c: u64,
}

/// El sistema de archivos de dos en la caja de DashOS (en la de Linux, `linux/archivos.rs`).
pub type Fs = FsDashos;

/// La sesión de la caja con el servicio de archivos. Sin servicio (no hay llave, o no contesta), todo da
/// `ErrorDelDisco`: los programas ven que el volumen no está.
pub struct FsDashos {
    canal: u32,
    lectura: u32,
    ventana: usize,
    tam_ventana: usize,
    pub ordenes: u32,
    /// Sin escribir nada: lo que cambia el disco da `SoloLectura` aquí, sin llegar al servicio (el volumen `SYS:` de la
    /// caja; la ficha de `inicio` dejaría escribir).
    pub solo_lectura: bool,
}

impl FsDashos {
    /// Sin servicio: todo da `ErrorDelDisco`.
    pub fn sin_servicio() -> FsDashos {
        FsDashos { canal: 0, lectura: 0, ventana: 0, tam_ventana: 0, ordenes: 0, solo_lectura: true }
    }

    /// Abre la sesión por la llave `canal`. `None` si el servicio no la da.
    pub fn abrir(canal: u32) -> Option<FsDashos> {
        let (lectura, vuelta) = dashos::canal_crear();
        let mut m = [0u8; p::CABECERA];
        m[0..4].copy_from_slice(&p::SESION.to_le_bytes());
        let mut escrito = false;
        for _ in 0..2000 {
            let e = dashos::canal_escribir(canal, &m, &[vuelta]);
            if e == 0 {
                escrito = true;
                break;
            }
            if e == dashos::E_CERRADO {
                break;
            }
            dashos::dormir_us(1_000);
        }
        if !escrito {
            dashos::cerrar(lectura);
            dashos::cerrar(vuelta);
            return None;
        }
        let mut f = FsDashos { canal, lectura, ventana: 0, tam_ventana: 0, ordenes: 0, solo_lectura: true };
        let mut ll = [0u32; 1];
        let (r, k) = f.respuesta(&mut ll)?;
        if r.estado != p::BIEN || k != 1 || r.a == 0 || r.a as usize > p::VENTANA_MAX {
            return None;
        }
        if dashos::memoria_proyectar(ll[0], VA_VENTANA, true) != 0 {
            return None;
        }
        f.ventana = VA_VENTANA;
        f.tam_ventana = r.a as usize;
        Some(f)
    }

    fn ventana(&self) -> &[u8] {
        // SEGURIDAD: la ventana está proyectada en `VA_VENTANA` con `tam_ventana` bytes desde `abrir` y no se quita.
        // El servicio la escribe solo mientras atiende una orden nuestra, y esperamos su respuesta antes de leerla.
        unsafe { slice::from_raw_parts(self.ventana as *const u8, self.tam_ventana) }
    }

    fn ventana_mut(&mut self) -> &mut [u8] {
        // SEGURIDAD: como en `ventana`.
        unsafe { slice::from_raw_parts_mut(self.ventana as *mut u8, self.tam_ventana) }
    }

    fn respuesta(&self, llaves: &mut [u32]) -> Option<(Respuesta, usize)> {
        let mut b = [0u8; 32];
        // Como mucho 10 s: un servicio que no contesta no deja colgada la caja.
        let (n, k, _) = dashos::canal_leer_plazo(self.lectura, &mut b, llaves, 10_000_000);
        if n != 32 {
            return None;
        }
        let u32_en = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let u64_en = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        Some((Respuesta { estado: u32_en(0) as i32, asa: u32_en(4), a: u64_en(8), b: u64_en(16), c: u64_en(24) }, k))
    }

    /// Una orden y su respuesta. El nombre ya viene limpio de dos (hasta 224 bytes).
    fn pedir(&mut self, orden: u32, asa: u32, a: u64, b: u64, nombre: &[u8]) -> Result<Respuesta, ErrorFs> {
        if self.ventana == 0 {
            return Err(ErrorFs::ErrorDelDisco);
        }
        if self.solo_lectura
            && matches!(orden, p::ESCRIBIR | p::CREAR | p::BORRAR | p::RENOMBRAR | p::TAMANO | p::COMENTAR | p::PROTEGER)
        {
            return Err(ErrorFs::SoloLectura);
        }
        if nombre.len() > 256 - p::CABECERA {
            return Err(ErrorFs::MalFormado);
        }
        self.ordenes += 1;
        let mut m = [0u8; 256];
        m[0..4].copy_from_slice(&orden.to_le_bytes());
        m[4..8].copy_from_slice(&asa.to_le_bytes());
        m[8..16].copy_from_slice(&a.to_le_bytes());
        m[16..24].copy_from_slice(&b.to_le_bytes());
        m[24..28].copy_from_slice(&(nombre.len() as u32).to_le_bytes());
        m[32..32 + nombre.len()].copy_from_slice(nombre);
        let mut escrito = false;
        for _ in 0..2000 {
            let e = dashos::canal_escribir(self.canal, &m[..32 + nombre.len()], &[]);
            if e == 0 {
                escrito = true;
                break;
            }
            if e == dashos::E_CERRADO {
                break;
            }
            dashos::dormir_us(1_000);
        }
        if !escrito {
            return Err(ErrorFs::ErrorDelDisco);
        }
        let mut ll = [0u32; 4];
        let (r, k) = self.respuesta(&mut ll).ok_or(ErrorFs::ErrorDelDisco)?;
        // Ninguna orden de las que manda dos devuelve llaves: si llegan, se sueltan.
        for &l in &ll[..k.min(4)] {
            dashos::cerrar(l);
        }
        if r.estado < 0 {
            return Err(ErrorFs::de_estado(r.estado).unwrap_or(ErrorFs::ErrorDelDisco));
        }
        Ok(r)
    }
}

fn datos(r: &Respuesta) -> Datos {
    Datos::de_respuesta(r.a, r.b, r.c)
}

impl SistemaArchivos for FsDashos {
    fn buscar(&mut self, asa: Asa, camino: &str) -> Result<(Asa, Datos), ErrorFs> {
        let r = self.pedir(p::BUSCAR, asa, 0, 0, camino.as_bytes())?;
        Ok((r.asa, datos(&r)))
    }

    fn mirar(&mut self, asa: Asa, camino: &str) -> Result<Datos, ErrorFs> {
        let r = self.pedir(p::BUSCAR, asa, 1, 0, camino.as_bytes())?;
        Ok(datos(&r))
    }

    fn leer(&mut self, asa: Asa, desde: u64, buf: &mut [u8]) -> Result<usize, ErrorFs> {
        let mut hecho = 0;
        while hecho < buf.len() {
            let n = (buf.len() - hecho).min(self.tam_ventana);
            let r = self.pedir(p::LEER, asa, desde + hecho as u64, n as u64, &[])?;
            let leidos = (r.a as usize).min(n);
            buf[hecho..hecho + leidos].copy_from_slice(&self.ventana()[..leidos]);
            hecho += leidos;
            if leidos < n {
                break;
            }
        }
        Ok(hecho)
    }

    fn escribir(&mut self, asa: Asa, desde: u64, datos: &[u8]) -> Result<usize, ErrorFs> {
        let mut hecho = 0;
        while hecho < datos.len() {
            let n = (datos.len() - hecho).min(self.tam_ventana);
            self.ventana_mut()[..n].copy_from_slice(&datos[hecho..hecho + n]);
            let r = self.pedir(p::ESCRIBIR, asa, desde + hecho as u64, n as u64, &[])?;
            let escritos = (r.a as usize).min(n);
            hecho += escritos;
            if escritos < n {
                break;
            }
        }
        Ok(hecho)
    }

    fn listar(&mut self, asa: Asa, desde: u64) -> Result<Pagina, ErrorFs> {
        let r = self.pedir(p::LISTAR, asa, desde, 0, &[])?;
        let v = self.ventana();
        let usados = (r.c as usize).min(v.len());
        let mut entradas = Vec::new();
        let mut off = 0;
        for _ in 0..r.a {
            if off + 24 > usados {
                break;
            }
            let e = &v[off..usados];
            let tamano = u64::from_le_bytes(e[0..8].try_into().unwrap());
            let fecha = u64::from_le_bytes(e[8..16].try_into().unwrap());
            let modo = u16::from_le_bytes([e[16], e[17]]);
            let tipo = e[18];
            let ln = e[19] as usize;
            let amiga = u16::from_le_bytes([e[20], e[21]]);
            let lc = e[22] as usize;
            if 24 + ln + lc > e.len() {
                break;
            }
            let nombre = String::from_utf8_lossy(&e[24..24 + ln]).into_owned();
            let comentario = String::from_utf8_lossy(&e[24 + ln..24 + ln + lc]).into_owned();
            let c = modo as u64 | (tipo as u64) << 16 | (amiga as u64) << 32;
            entradas.push(Entrada { nombre, comentario, datos: Datos::de_respuesta(tamano, fecha, c) });
            off += (24 + ln + lc).next_multiple_of(8);
        }
        Ok(Pagina { entradas, siguiente: if r.b == u64::MAX { None } else { Some(r.b) } })
    }

    fn datos(&mut self, asa: Asa) -> Result<Datos, ErrorFs> {
        let r = self.pedir(p::DATOS, asa, 0, 0, &[])?;
        Ok(datos(&r))
    }

    fn cerrar(&mut self, asa: Asa) -> Result<(), ErrorFs> {
        self.pedir(p::CERRAR, asa, 0, 0, &[]).map(|_| ())
    }

    fn crear(&mut self, asa: Asa, nombre: &str, cajon: bool) -> Result<(Asa, Datos), ErrorFs> {
        let r = self.pedir(p::CREAR, asa, if cajon { 2 } else { 1 }, 0, nombre.as_bytes())?;
        Ok((r.asa, datos(&r)))
    }

    fn borrar(&mut self, asa: Asa, nombre: &str) -> Result<(), ErrorFs> {
        self.pedir(p::BORRAR, asa, 0, 0, nombre.as_bytes()).map(|_| ())
    }

    fn renombrar(&mut self, asa: Asa, viejo: &str, asa_nueva: Asa, nuevo: &str) -> Result<(), ErrorFs> {
        let mut n = Vec::with_capacity(viejo.len() + 1 + nuevo.len());
        n.extend_from_slice(viejo.as_bytes());
        n.push(0);
        n.extend_from_slice(nuevo.as_bytes());
        self.pedir(p::RENOMBRAR, asa, 0, asa_nueva as u64, &n).map(|_| ())
    }

    fn cambiar_tamano(&mut self, asa: Asa, tamano: u64) -> Result<u64, ErrorFs> {
        self.pedir(p::TAMANO, asa, tamano, 0, &[]).map(|r| r.a)
    }

    fn vaciar(&mut self) -> Result<(), ErrorFs> {
        self.pedir(p::VACIAR, 0, 0, 0, &[]).map(|_| ())
    }

    fn info(&mut self, asa: Asa) -> Result<InfoVolumen, ErrorFs> {
        let r = self.pedir(p::INFO, asa, 0, 0, &[])?;
        Ok(InfoVolumen { tamano: r.a, libre: r.b, bloque: r.c as u32 })
    }

    fn comentario(&mut self, asa: Asa) -> Result<String, ErrorFs> {
        let r = self.pedir(p::COMENTARIO, asa, 0, 0, &[])?;
        let n = (r.a as usize).min(255).min(self.tam_ventana);
        Ok(String::from_utf8_lossy(&self.ventana()[..n]).into_owned())
    }

    fn comentar(&mut self, asa: Asa, comentario: &str) -> Result<(), ErrorFs> {
        self.pedir(p::COMENTAR, asa, 0, 0, comentario.as_bytes()).map(|_| ())
    }

    fn proteger(&mut self, asa: Asa, amiga: u16, modo: u16) -> Result<(), ErrorFs> {
        self.pedir(p::PROTEGER, asa, amiga as u64, modo as u64, &[]).map(|_| ())
    }
}
