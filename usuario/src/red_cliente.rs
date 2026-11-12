//! El lado del programa del servicio de red (S13): abrir una conexión TCP, enviar, recibir y cerrar, con los anillos
//! de `protocolo::red`. Sin hilos ni montón: cada conexión es su memoria compartida y un canal propio por el que el
//! servicio contesta y avisa.

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

fn palabras(p: &[u32]) -> &[u8] {
    // Un `&[u32]` se puede ver como bytes: misma memoria, alineación menor.
    unsafe { core::slice::from_raw_parts(p.as_ptr() as *const u8, 4 * p.len()) }
}

/// Manda `ESTADO` y devuelve sus palabras (`protocolo::red::E_*`), o el error.
pub fn estado(servicio: u32, plazo_us: u64) -> Result<[u32; p::E_PALABRAS], i64> {
    let (lee, escribe) = crate::canal_crear();
    let copia = crate::llave_duplicar(escribe, crate::ESCRIBIR | crate::TRANSFERIR, 0);
    let r = if copia <= 0 {
        Err(copia)
    } else {
        let e = crate::canal_escribir(servicio, palabras(&[p::ESTADO]), &[copia as u32]);
        if e != 0 {
            crate::cerrar(copia as u32);
            Err(e)
        } else {
            let mut b = [0u32; p::E_PALABRAS];
            let mut ll = [0u32; 8];
            let bytes = unsafe { core::slice::from_raw_parts_mut(b.as_mut_ptr() as *mut u8, 4 * b.len()) };
            let (n, k, _) = crate::canal_leer_plazo(lee, bytes, &mut ll, plazo_us);
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
        let (lee, escribe) = crate::canal_crear();
        let c = Conexion { servicio, id: 0, va, tam, memoria: 0, lee, escribe };
        let e = crate::llave_duplicar(escribe, crate::ESCRIBIR | crate::TRANSFERIR, 0);
        if e <= 0 {
            c.soltar();
            return Err(p::LIMITE);
        }
        let ipw = u32::from_le_bytes(ip);
        let peticion = [p::CONECTAR, p::IPV4, puerto as u32, tam as u32, ipw, 0, 0, 0];
        if crate::canal_escribir(servicio, palabras(&peticion), &[e as u32]) != 0 {
            crate::cerrar(e as u32);
            c.soltar();
            return Err(p::SIN_DIRECCION);
        }
        let mut b = [0u32; 4];
        let mut ll = [0u32; 8];
        let fin = crate::ahora_us() + plazo_us;
        loop {
            let ahora = crate::ahora_us();
            if ahora >= fin {
                c.soltar();
                return Err(p::RECHAZADA);
            }
            let bytes = unsafe { core::slice::from_raw_parts_mut(b.as_mut_ptr() as *mut u8, 16) };
            let (n, k, _) = crate::canal_leer_plazo(lee, bytes, &mut ll, fin - ahora);
            let k = if n >= 0 { k.min(ll.len()) } else { 0 };
            if n >= 12 && b[0] == p::R_CONECTAR {
                if b[2] as i32 != p::BIEN || k != 1 {
                    ll[..k].iter().for_each(|&l| {
                        crate::cerrar(l);
                    });
                    c.soltar();
                    return Err(if b[2] as i32 != p::BIEN { b[2] as i32 } else { p::MAL_FORMADO });
                }
                let memoria = ll[0];
                let tamano = crate::memoria_tamano(memoria);
                if tamano < (p::DATOS + 2 * tam) as i64 || crate::memoria_proyectar(memoria, va, true) != 0 {
                    crate::cerrar(memoria);
                    c.soltar();
                    return Err(p::MAL_FORMADO);
                }
                return Ok(Conexion { id: b[1], memoria, ..c });
            }
            ll[..k].iter().for_each(|&l| {
                crate::cerrar(l);
            });
        }
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
        unsafe { read_volatile((self.va + 4 * i) as *const u32) }
    }

    fn poner(&self, i: usize, v: u32) {
        unsafe { write_volatile((self.va + 4 * i) as *mut u32, v) }
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
            crate::canal_escribir(self.servicio, palabras(&[p::AVISO, self.id]), &[]);
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
        crate::canal_escribir(self.servicio, palabras(&[p::CERRAR, self.id]), &[]);
        self.soltar();
    }
}
