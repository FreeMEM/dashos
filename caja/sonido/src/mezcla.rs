//! El sondeo de la caja (`atender`): mezcla por delante de lo que suena, devuelve lo que ya sonó y pide los ganchos.
//!
//! Se mezcla en trozos de hasta `TROZO` cuadros (5 ms), y nunca más allá de `adelanto` por delante de lo que suena:
//! así una escritura vuelve a tiempo (cuando su último cuadro ha sonado, no al mezclarlo) y un cambio del programa
//! se oye pronto. Cada trozo acaba en el siguiente tic de un jugador (`AHIA_PlayerFunc`): la mezcla se para, la caja
//! ejecuta el gancho y la mezcla sigue con lo que haya cambiado, como hace AHI. Un sonido que empieza
//! (`AHIA_SoundFunc`) también para la mezcla al acabar su trozo (más corto con un `SoundFunc`: 2 ms).
//!
//! Qué es "lo que suena": con `Salida::pendientes`, lo escrito menos lo que aún espera en la salida (el reloj del
//! sonido); sin ella, el reloj de `ahora_us`, que se reengancha si la salida se quedó sin nada (un hueco que ya ha
//! sonado no se recupera).

use caja_exec::{Exec, Fallo, Memoria};

use crate::mem::Lectura;
use crate::*;

/// Lo que se mezcla por delante de lo que suena: 40 ms (el búfer del servicio es de 40 a 80 ms).
pub const ADELANTO_US: u64 = 40_000;
/// El trozo más largo que se mezcla de una vez: 240 cuadros (5 ms).
pub const TROZO: usize = 240;
/// El trozo con algún `SoundFunc`: 96 cuadros (2 ms).
const TROZO_SONIDO: usize = 96;
/// Un gancho que no vuelve en este tiempo se da por perdido (la caja no llamó a `gancho_hecho`).
const MAX_GANCHO_US: u64 = 1_000_000;

pub(crate) fn cuadros_de_us(us: u64) -> u64 {
    us * FRECUENCIA as u64 / 1_000_000
}

impl Sonido {
    /// Cuánto se mezcla por delante de lo que suena (entre 5 y 500 ms).
    pub fn poner_adelanto(&mut self, us: u64) {
        self.adelanto = cuadros_de_us(us.clamp(5_000, 500_000));
    }

    /// Los cuadros escritos en la salida desde que se instaló.
    pub fn producidos(&self) -> u64 {
        self.producidos
    }

    /// El gancho que dio `atender` ya volvió: la mezcla sigue en el siguiente `atender`.
    pub fn gancho_hecho(&mut self) {
        self.en_gancho = None;
    }

    /// Lo que ya ha sonado, en cuadros.
    fn sonados<S: Salida>(&self, salida: &S, ahora_us: u64) -> u64 {
        if let Some(p) = salida.pendientes() {
            return self.producidos.saturating_sub(p as u64);
        }
        match self.reloj {
            Some((t0, base)) => base + cuadros_de_us(ahora_us.saturating_sub(t0)),
            None => self.producidos,
        }
    }

    /// Si hay algo que mezclar (aunque sea silencio: una `AHIAudioCtrl` tocando sin canales).
    fn suena(&self) -> bool {
        self.escrituras.values().any(|e| e.voz.is_some())
            || self.controles.values().any(|c| c.tocando)
            || (0..4).any(|i| self.paula.sonando(i).is_some())
    }

    /// El sondeo de la caja, en su bucle de espera y en cada llamada nativa (`ahora_us`, el reloj de DashOS):
    /// devuelve las peticiones que ya sonaron (`ReplyMsg`) y mezcla lo que quepa en `salida`, hasta `adelanto` por
    /// delante. Si toca un gancho y `ganchos` es `true`, lo devuelve: la caja lo ejecuta, llama a `gancho_hecho` y
    /// vuelve a llamar (la mezcla sigue tras él). Con `ganchos` `false` (la caja no puede ejecutar 68k ahora), la
    /// mezcla se para antes del gancho hasta que se le pueda dar. Dentro de un gancho (antes de `gancho_hecho`), solo
    /// devuelve lo que sonó: un gancho que llama a ahi.device no mezcla por debajo.
    pub fn atender<M: Memoria, S: Salida>(
        &mut self,
        ex: &mut Exec<M>,
        salida: &mut S,
        ahora_us: u64,
        ganchos: bool,
    ) -> Option<Gancho> {
        if let Some(desde) = self.en_gancho {
            if ahora_us.saturating_sub(desde) < MAX_GANCHO_US {
                self.devolver(ex, salida, ahora_us);
                return None;
            }
            self.avisar(ex, "(gancho de AHI)", Fallo::Sospechoso { dir: 0 });
            self.en_gancho = None;
        }
        self.devolver(ex, salida, ahora_us);
        // Un tope de vueltas: un trozo de 1 cuadro por gancho, hasta el adelanto entero, como mucho.
        for _ in 0..(self.adelanto as usize + 64) {
            if ganchos {
                while let Some(p) = self.ganchos.pop_front() {
                    if let Some(g) = self.gancho_de(ex, p) {
                        self.en_gancho = Some(ahora_us);
                        return Some(g);
                    }
                }
            }
            if !self.ganchos.is_empty() || !self.mezclar_trozo(ex, salida, ahora_us) {
                break;
            }
        }
        None
    }

    /// Devuelve lo que ya sonó, por orden.
    fn devolver<M: Memoria, S: Salida>(&mut self, ex: &mut Exec<M>, salida: &S, ahora_us: u64) {
        let s = self.sonados(salida, ahora_us);
        while let Some(v) = self.vueltas.front() {
            if v.cuadro > s {
                break;
            }
            let v = self.vueltas.pop_front().unwrap();
            match v.que {
                Que::Flujo { id } => self.devolver_flujo(ex, id),
                Que::Paula(a) => {
                    let _ = self.aplicar_paula(ex, a);
                }
            }
        }
    }

    /// Una vuelta en su sitio, por orden de cuadro (y por orden de llegada entre las del mismo).
    pub(crate) fn poner_vuelta(&mut self, v: Vuelta) {
        let k = self.vueltas.iter().rposition(|x| x.cuadro <= v.cuadro).map_or(0, |k| k + 1);
        self.vueltas.insert(k, v);
    }

    /// Los tics del jugador que tocan ya: se pide su gancho (uno por `AHIAudioCtrl`, aunque se hayan perdido varios).
    fn pedir_tics(&mut self) {
        let p = self.producidos;
        for (&d, c) in self.controles.iter_mut() {
            if !c.tocando || c.jugador_hz == 0 || c.jugador_func == 0 || (c.siguiente_tic >> 16) > p {
                continue;
            }
            let periodo = c.periodo_tic();
            while (c.siguiente_tic >> 16) <= p {
                c.siguiente_tic += periodo;
            }
            let pedido = Pedido { control: d, canal: None };
            if !self.ganchos.contains(&pedido) {
                self.ganchos.push_back(pedido);
            }
        }
    }

    /// Mezcla un trozo. `false` si no había nada que hacer (ya va bastante por delante, la salida está llena o no
    /// suena nada).
    fn mezclar_trozo<M: Memoria, S: Salida>(&mut self, ex: &mut Exec<M>, salida: &mut S, ahora_us: u64) -> bool {
        let reloj = salida.pendientes().is_none();
        if !self.suena() {
            if reloj && self.vueltas.is_empty() {
                self.reloj = None;
            }
            return false;
        }
        if reloj {
            // Al empezar, o tras quedarse la salida sin nada: lo que suena es lo que se va a escribir ahora.
            let s = self.sonados(salida, ahora_us);
            if self.reloj.is_none() || s > self.producidos {
                self.reloj = Some((ahora_us, self.producidos));
            }
        }
        let objetivo = self.sonados(salida, ahora_us) + self.adelanto;
        if self.producidos >= objetivo {
            return false;
        }
        let hueco = salida.hueco();
        if hueco == 0 {
            return false;
        }
        self.pedir_tics();
        if !self.ganchos.is_empty() {
            return true;
        }
        let tope = if self.controles.values().any(|c| c.tocando && c.sonido_func != 0) { TROZO_SONIDO } else { TROZO };
        let mut n = ((objetivo - self.producidos) as usize).min(hueco).min(tope);
        for c in self.controles.values() {
            if c.tocando && c.jugador_hz != 0 && c.jugador_func != 0 {
                n = n.min(((c.siguiente_tic >> 16) - self.producidos).max(1) as usize);
            }
        }
        self.buf.clear();
        self.buf.resize(n, [0, 0]);
        self.tmp.resize(n, [0, 0]);
        let (buf, tmp) = (&mut self.buf, &mut self.tmp);
        let sumar = |buf: &mut [[i16; 2]], tmp: &[[i16; 2]]| {
            for (b, t) in buf.iter_mut().zip(tmp) {
                b[0] = b[0].saturating_add(t[0]);
                b[1] = b[1].saturating_add(t[1]);
            }
        };
        let mem = Lectura(&ex.mem);
        let (p0, h0, hp0) = (self.producidos, self.flujo.mezclados(), self.paula.mezclados());
        if !self.escrituras.is_empty() {
            self.flujo.mezclar(&mut tmp[..n]);
            sumar(buf, &tmp[..n]);
        }
        for c in self.controles.values_mut().filter(|c| c.tocando) {
            c.ahi.mezclar_con(&mut tmp[..n], &mem);
            sumar(buf, &tmp[..n]);
        }
        if (0..4).any(|i| self.paula.sonando(i).is_some()) {
            self.paula.mezclar_con(&mut tmp[..n], &mem);
            sumar(buf, &tmp[..n]);
        }
        salida.escribir(&self.buf[..n]);
        self.producidos += n as u64;
        self.recoger_flujo(p0, h0);
        for (a, c) in self.paula.avisos_con_cuadro() {
            self.poner_vuelta(Vuelta { cuadro: p0 + c.saturating_sub(hp0), que: Que::Paula(a) });
        }
        let controles: alloc::vec::Vec<u32> = self.controles.keys().copied().collect();
        for d in controles {
            self.recoger_control(d);
        }
        true
    }
}
