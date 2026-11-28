//! El transporte de Wayland del compositor (`compositor.rs`) en DashOS: el canal "ventanas" (`wayland::canal`, W2), con
//! el flujo en trozos y las llaves de DashOS. En Linux, el socket de Unix del compositor (`linux/transporte.rs`). Lo
//! mismo en los dos: mandar lo de un `Escritor` (sus llaves dejan de ser nuestras), recoger lo que llegue en el
//! juntador, y la memoria compartida de los búferes.

use alloc::vec::Vec;

use wayland::Escritor;
use wayland::canal::{self, Juntador, TROZO_CONECTAR, TROZO_DATOS};

/// Lo que dio `recoger`.
pub enum Recogido {
    Nada,
    Algo,
    /// El compositor cerró la conexión (con el código de DashOS).
    Cerrado(i64),
}

pub struct Transporte {
    llave: u32,
    respuesta: u32,
    juntador: Juntador,
}

impl Transporte {
    /// Sin conexión.
    pub fn nada() -> Transporte {
        Transporte { llave: 0, respuesta: 0, juntador: Juntador::new() }
    }

    /// Se conecta al compositor por la llave `llave` (el canal "ventanas" que da `inicio`).
    pub fn conectar(llave: u32) -> Option<Transporte> {
        let (mio, suyo) = dashos::canal_crear();
        let mut r = -1;
        for _ in 0..5_000 {
            r = dashos::canal_escribir(llave, &[TROZO_CONECTAR], &[suyo]);
            if r == 0 || r == dashos::E_CERRADO {
                break;
            }
            dashos::dormir_us(1_000);
        }
        if r != 0 {
            dashos::cerrar(mio);
            dashos::cerrar(suyo);
            return None;
        }
        Some(Transporte { llave, respuesta: mio, juntador: Juntador::new() })
    }

    /// Manda los mensajes de `e`, con sus llaves (que pasan al compositor). `false` si no se pudo: la conexión se da
    /// por perdida y las llaves se sueltan.
    pub fn mandar(&mut self, e: &Escritor) -> bool {
        let Ok(trozos) = canal::trocear(e) else { return true };
        for t in trozos {
            let mut hecho = false;
            for _ in 0..2000 {
                let r = dashos::canal_escribir(self.llave, &t.bytes, &t.llaves);
                if r == 0 {
                    hecho = true;
                    break;
                }
                if r == dashos::E_CERRADO {
                    break;
                }
                // Con su cola llena, el compositor puede estar esperando a que leamos: se recoge lo suyo.
                if !matches!(self.recoger(1), Recogido::Algo) {
                    dashos::dormir_us(1_000);
                }
            }
            if !hecho {
                for l in &t.llaves {
                    dashos::cerrar(*l);
                }
                return false;
            }
        }
        true
    }

    /// Lee un trozo del compositor, esperando como mucho `us`.
    pub fn recoger(&mut self, us: u64) -> Recogido {
        let mut buf = [0u8; canal::MAX_CANAL];
        let mut ll = [0u32; canal::MAX_LLAVES];
        let (n, k, _) = dashos::canal_leer_plazo(self.respuesta, &mut buf, &mut ll, us.max(1));
        if n == dashos::E_PLAZO {
            return Recogido::Nada;
        }
        if n <= 0 {
            return Recogido::Cerrado(n);
        }
        if buf[0] == TROZO_DATOS && self.juntador.meter(&buf[1..n as usize], &ll[..k]).is_ok() {
            return Recogido::Algo;
        }
        for &l in &ll[..k] {
            dashos::cerrar(l);
        }
        Recogido::Algo
    }

    /// El siguiente mensaje entero que haya llegado: objeto, orden y cuerpo.
    pub fn siguiente(&mut self) -> Option<(u32, u16, Vec<u8>)> {
        match self.juntador.siguiente() {
            Ok(Some(m)) => Some((m.objeto, m.orden, m.cuerpo)),
            _ => None,
        }
    }

    /// La llave que llegó con el último mensaje (la del mapa del teclado...).
    pub fn tomar_llave(&mut self) -> Option<u32> {
        self.juntador.tomar_llave()
    }

    pub fn cerrar(l: u32) {
        dashos::cerrar(l);
    }

    /// Memoria nueva de `tam` bytes, proyectada para escribir en `va`: su llave y la que se manda al compositor (una
    /// copia de solo lectura, que se va con el mensaje).
    pub fn memoria(&mut self, tam: usize, va: usize) -> Option<(u32, u32)> {
        let m = dashos::memoria_crear(tam);
        if (m as i32) <= 0 {
            return None;
        }
        let copia = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
        if copia <= 0 || dashos::memoria_proyectar(m, va, true) != 0 {
            if copia > 0 {
                dashos::cerrar(copia as u32);
            }
            dashos::cerrar(m);
            return None;
        }
        Some((m, copia as u32))
    }

    /// Suelta la memoria de `memoria`, proyectada en `va`.
    pub fn soltar_memoria(&mut self, memoria: u32, va: usize) {
        dashos::memoria_quitar(va);
        dashos::cerrar(memoria);
    }
}
