//! El grafo del sonido (S19b, fase E1): el motor del servicio deja de ser un mezclador y pasa a ser un grafo, como
//! JACK.
//!
//! - **Nodos**: el flujo de un programa (de él sale audio), una mezcla (suma lo que le entra), un efecto (procesa lo
//!   que le entra) y una salida (lo que le entra va al aparato). Cada nodo tiene una entrada y una salida de audio
//!   estéreo (puerto 0); los puertos de MIDI (E3) y los de varios canales llegarán con lo que los pida.
//! - **Conexiones** libres entre la salida de un nodo y la entrada de otro, que se cambian sin cortar el sonido: una
//!   conexión nueva entra en rampa a lo largo de un cuanto y una que se quita sale igual. Mover un flujo de una salida
//!   a otra es quitar una y poner otra: un fundido cruzado. Se rechaza lo que haría un ciclo.
//! - **Orden por dependencias**: cada cuanto se procesan los nodos en orden topológico (Kahn), cada uno una vez,
//!   cuando sus entradas están listas. El orden se recalcula solo cuando cambia el grafo.
//! - **Un cuanto para todo el grafo y un período por salida**: el grafo corre en cuantos de `cuanto` cuadros (el
//!   período más corto de las salidas: 64 para tocar, 256 normal, 1024 para ahorrar). Cada salida junta lo que le
//!   llega en su cola, y su aparato se lleva períodos de su tamaño. Así una salida de 64 y otra de 256 conviven: la de
//!   256 recibe cuatro cuantos por período.
//!
//! Cada nodo lleva su ganancia (izquierda y derecha) y su silencio, en rampa, y su medidor (pico y nivel cuadrático
//! medio de lo que sale, ya con la ganancia), que se lee y se pone a cero: son los vúmetros.
//!
//! Todo en coma fija, como la mezcla de antes (E9): muestras con 1,0 = `UNO`, ganancias con 1,0 = 65.536.
//!
//! Lo que no hace aún (apuntado en `docs/bitacoras/sonido-pro.md`): nodos independientes en paralelo en varios
//! núcleos, cambios de parámetro con su muestra exacta dentro del cuanto (hoy en rampa por cuanto) y puertos MIDI.

use crate::efectos::Efecto;
use crate::{a_volumen, cabida, convertir, Cuadro, Formato, Remuestreo, UNO_VOL};
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

pub type Id = u32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clase {
    Flujo,
    Mezcla,
    Efecto,
    Salida,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    NoExiste,
    /// La conexión haría un ciclo.
    Ciclo,
    /// Un nodo sin entrada (un flujo) no puede recibir, ni una salida dar.
    Puerto,
    /// Ya están conectados.
    Repetida,
    /// No caben más nodos o conexiones.
    Lleno,
    /// El formato de un flujo no vale.
    Formato,
}

/// Lo más que cabe en el grafo: un programa con mil flujos no lo agota sin que el servicio le pare antes.
pub const MAX_NODOS: usize = 256;
pub const MAX_CONEXIONES: usize = 1024;
/// Los cuantos que valen.
pub const CUANTO_MIN: usize = 16;
pub const CUANTO_MAX: usize = 4096;

/// La medida de lo que sale de un nodo desde la última lectura.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Medida {
    /// El pico de cada lado (1,0 = `UNO`).
    pub pico: [i32; 2],
    /// El nivel cuadrático medio de cada lado (1,0 = `UNO`).
    pub rms: [i32; 2],
}

#[derive(Default)]
struct Medidor {
    pico: [i32; 2],
    suma: [u64; 2],
    cuadros: u64,
}

impl Medidor {
    fn medir(&mut self, buf: &[Cuadro]) {
        for c in buf {
            for k in 0..2 {
                let a = c[k].unsigned_abs().min(i32::MAX as u32) as i32;
                self.pico[k] = self.pico[k].max(a);
                // Al cuadrado con 15 bits menos (sin desbordar en un segundo de muestras grandes).
                let x = (c[k] as i64).clamp(-(8 << 15), 8 << 15);
                self.suma[k] = self.suma[k].saturating_add(((x * x) >> 8) as u64);
            }
        }
        self.cuadros += buf.len() as u64;
    }

    fn leer(&mut self) -> Medida {
        let mut rms = [0; 2];
        if self.cuadros > 0 {
            for k in 0..2 {
                rms[k] = raiz_entera((self.suma[k] / self.cuadros) << 8) as i32;
            }
        }
        let m = Medida { pico: self.pico, rms };
        *self = Medidor::default();
        m
    }
}

fn raiz_entera(x: u64) -> u64 {
    if x == 0 {
        return 0;
    }
    let mut r = 1u64 << ((64 - x.leading_zeros()).div_ceil(2));
    loop {
        let s = (r + x / r) / 2;
        if s >= r {
            return r;
        }
        r = s;
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Fase {
    Suena,
    /// Saliendo: le quedan estos cuadros de fundido; a cero, se quita.
    Saliendo(u32),
}

/// Un flujo de un programa.
struct Fuente {
    formato: Formato,
    cola: VecDeque<Cuadro>,
    remuestreo: Remuestreo,
    empezado: bool,
    faltas: u64,
}

/// Una salida: lo que le llega se junta en su cola hasta que el aparato se lo lleva.
struct Destino {
    periodo: usize,
    cola: VecDeque<Cuadro>,
    /// Cuadros que se tiraron porque nadie se los llevó (la cola pasó de un segundo).
    desbordes: u64,
}

enum Cuerpo {
    Fuente(Fuente),
    Mezcla,
    Efecto { efecto: Box<dyn Efecto>, activo: bool, mezcla: i64, seco: Vec<Cuadro> },
    Destino(Destino),
}

struct Nodo {
    id: Id,
    nombre: String,
    /// De quién es (el servicio pone aquí la marca del programa; 0, del sistema).
    dueno: u64,
    cuerpo: Cuerpo,
    /// La ganancia pedida (con el silencio ya aplicado) y la del final del último cuanto.
    volumen: [i64; 2],
    silencio: bool,
    actual: [i64; 2],
    fase: Fase,
    buf: Vec<Cuadro>,
    medidor: Medidor,
    /// Lo que salió en el último cuanto era silencio (todo ceros): con la entrada también en silencio, un efecto no
    /// tiene nada que hacer (su cola ya se apagó) y se salta (el grafo en reposo casi no gasta).
    callado: bool,
}

impl Nodo {
    fn clase(&self) -> Clase {
        match self.cuerpo {
            Cuerpo::Fuente(_) => Clase::Flujo,
            Cuerpo::Mezcla => Clase::Mezcla,
            Cuerpo::Efecto { .. } => Clase::Efecto,
            Cuerpo::Destino(_) => Clase::Salida,
        }
    }

    fn pedida(&self) -> [i64; 2] {
        if self.silencio { [0; 2] } else { self.volumen }
    }
}

struct Conexion {
    de: Id,
    a: Id,
    /// 0 a `UNO_VOL`: la que se pide (0 si se está quitando) y la del final del último cuanto.
    pedida: i64,
    actual: i64,
}

/// Un paso del plan: qué nodo y de qué conexiones (por su sitio) y nodos (por su sitio) le llega lo suyo.
struct Paso {
    nodo: usize,
    entradas: Vec<(usize, usize)>,
}

/// Lo que se sabe de un nodo desde fuera.
#[derive(Clone, Debug, PartialEq)]
pub struct Ficha {
    pub id: Id,
    pub clase: Clase,
    pub nombre: String,
    pub dueno: u64,
    pub volumen: [i64; 2],
    pub silencio: bool,
    /// Lo que se le pidió quitar: está saliendo con su fundido.
    pub saliendo: bool,
}

pub struct Grafo {
    frecuencia: u32,
    cuanto: usize,
    nodos: Vec<Nodo>,
    conexiones: Vec<Conexion>,
    plan: Vec<Paso>,
    sucio: bool,
    siguiente: Id,
    entrada: Vec<Cuadro>,
}

impl Grafo {
    pub fn nuevo(frecuencia: u32, cuanto: usize) -> Grafo {
        Grafo {
            frecuencia,
            cuanto: cuanto.clamp(CUANTO_MIN, CUANTO_MAX),
            nodos: Vec::new(),
            conexiones: Vec::new(),
            plan: Vec::new(),
            sucio: true,
            siguiente: 1,
            entrada: Vec::new(),
        }
    }

    pub fn frecuencia(&self) -> u32 {
        self.frecuencia
    }

    pub fn cuanto(&self) -> usize {
        self.cuanto
    }

    /// Cambia el cuanto (se recorta a lo que vale). Es seguro en cualquier momento: todo va por muestras.
    pub fn poner_cuanto(&mut self, cuadros: usize) {
        self.cuanto = cuadros.clamp(CUANTO_MIN, CUANTO_MAX);
    }

    /// Los cuadros que dura el fundido de un nodo que sale (5 ms).
    pub fn fundido(&self) -> u32 {
        (self.frecuencia / 200).max(1)
    }

    fn sitio(&self, id: Id) -> Option<usize> {
        self.nodos.iter().position(|n| n.id == id)
    }

    fn nodo(&mut self, id: Id) -> Option<&mut Nodo> {
        self.nodos.iter_mut().find(|n| n.id == id)
    }

    fn anadir(&mut self, nombre: &str, dueno: u64, cuerpo: Cuerpo, volumen_inicial: [i64; 2]) -> Result<Id, Error> {
        if self.nodos.len() >= MAX_NODOS {
            return Err(Error::Lleno);
        }
        let id = self.siguiente;
        self.siguiente = self.siguiente.wrapping_add(1).max(1);
        self.nodos.push(Nodo {
            id,
            nombre: String::from(nombre),
            dueno,
            cuerpo,
            volumen: [UNO_VOL; 2],
            silencio: false,
            actual: volumen_inicial,
            fase: Fase::Suena,
            buf: Vec::new(),
            medidor: Medidor::default(),
            callado: false,
        });
        self.sucio = true;
        Ok(id)
    }

    /// Un flujo de un programa, que entra subiendo desde cero.
    pub fn flujo(&mut self, nombre: &str, dueno: u64, formato: Formato) -> Result<Id, Error> {
        if !formato.valido() {
            return Err(Error::Formato);
        }
        let f = Fuente { formato, cola: VecDeque::new(), remuestreo: Remuestreo::nuevo(formato.frecuencia, self.frecuencia), empezado: false, faltas: 0 };
        self.anadir(nombre, dueno, Cuerpo::Fuente(f), [0; 2])
    }

    pub fn mezcla(&mut self, nombre: &str) -> Result<Id, Error> {
        self.anadir(nombre, 0, Cuerpo::Mezcla, [UNO_VOL; 2])
    }

    pub fn efecto(&mut self, nombre: &str, efecto: Box<dyn Efecto>) -> Result<Id, Error> {
        self.anadir(nombre, 0, Cuerpo::Efecto { efecto, activo: true, mezcla: UNO_VOL, seco: Vec::new() }, [UNO_VOL; 2])
    }

    /// Una salida con su período (cuadros que se lleva su aparato cada vez).
    pub fn salida(&mut self, nombre: &str, periodo: usize) -> Result<Id, Error> {
        let d = Destino { periodo: periodo.clamp(CUANTO_MIN, CUANTO_MAX), cola: VecDeque::new(), desbordes: 0 };
        self.anadir(nombre, 0, Cuerpo::Destino(d), [UNO_VOL; 2])
    }

    /// Conecta la salida de `de` con la entrada de `a`. La conexión entra en rampa.
    pub fn conectar(&mut self, de: Id, a: Id) -> Result<(), Error> {
        let (Some(i), Some(j)) = (self.sitio(de), self.sitio(a)) else { return Err(Error::NoExiste) };
        if self.nodos[i].clase() == Clase::Salida || self.nodos[j].clase() == Clase::Flujo {
            return Err(Error::Puerto);
        }
        if let Some(c) = self.conexiones.iter_mut().find(|c| c.de == de && c.a == a) {
            if c.pedida == 0 {
                // Se estaba quitando: vuelve, desde donde esté.
                c.pedida = UNO_VOL;
                return Ok(());
            }
            return Err(Error::Repetida);
        }
        if self.conexiones.len() >= MAX_CONEXIONES {
            return Err(Error::Lleno);
        }
        if de == a || self.llega(a, de) {
            return Err(Error::Ciclo);
        }
        self.conexiones.push(Conexion { de, a, pedida: UNO_VOL, actual: 0 });
        self.sucio = true;
        Ok(())
    }

    /// ¿Hay un camino de `desde` a `hasta`?
    fn llega(&self, desde: Id, hasta: Id) -> bool {
        let mut pila = alloc::vec![desde];
        let mut vistos: Vec<Id> = Vec::new();
        while let Some(n) = pila.pop() {
            if n == hasta {
                return true;
            }
            if vistos.contains(&n) {
                continue;
            }
            vistos.push(n);
            pila.extend(self.conexiones.iter().filter(|c| c.de == n).map(|c| c.a));
        }
        false
    }

    /// Quita la conexión: sale en rampa a lo largo del cuanto siguiente.
    pub fn desconectar(&mut self, de: Id, a: Id) -> Result<(), Error> {
        let c = self.conexiones.iter_mut().find(|c| c.de == de && c.a == a && c.pedida != 0).ok_or(Error::NoExiste)?;
        c.pedida = 0;
        Ok(())
    }

    /// Las conexiones vivas (sin las que se están quitando).
    pub fn conexiones(&self) -> Vec<(Id, Id)> {
        self.conexiones.iter().filter(|c| c.pedida != 0).map(|c| (c.de, c.a)).collect()
    }

    /// A dónde va lo que sale de `de` (conexiones vivas).
    pub fn destinos(&self, de: Id) -> Vec<Id> {
        self.conexiones.iter().filter(|c| c.de == de && c.pedida != 0).map(|c| c.a).collect()
    }

    /// Quita un nodo: sale con un fundido de 5 ms y se va con sus conexiones. A un flujo le suena antes lo que le
    /// quede en la cola durante el fundido.
    pub fn quitar(&mut self, id: Id) -> Result<(), Error> {
        let fundido = self.fundido();
        let n = self.nodo(id).ok_or(Error::NoExiste)?;
        if n.fase == Fase::Suena {
            n.fase = Fase::Saliendo(fundido);
        }
        Ok(())
    }

    pub fn existe(&self, id: Id) -> bool {
        self.sitio(id).is_some()
    }

    pub fn nodos(&self) -> usize {
        self.nodos.len()
    }

    pub fn ficha(&self, id: Id) -> Option<Ficha> {
        self.nodos.iter().find(|n| n.id == id).map(|n| Ficha {
            id: n.id,
            clase: n.clase(),
            nombre: n.nombre.clone(),
            dueno: n.dueno,
            volumen: n.volumen,
            silencio: n.silencio,
            saliendo: n.fase != Fase::Suena,
        })
    }

    pub fn fichas(&self) -> Vec<Ficha> {
        self.nodos.iter().filter_map(|n| self.ficha(n.id)).collect()
    }

    /// El volumen de cada lado (0 a 4; NaN es 0), en rampa a lo largo del cuanto siguiente.
    pub fn volumen(&mut self, id: Id, izquierdo: f32, derecho: f32) -> Result<(), Error> {
        let n = self.nodo(id).ok_or(Error::NoExiste)?;
        n.volumen = [a_volumen(izquierdo), a_volumen(derecho)];
        Ok(())
    }

    /// El volumen en coma fija (1,0 = 65.536), sin pasar de 4.
    pub fn volumen_fijo(&mut self, id: Id, v: [i64; 2]) -> Result<(), Error> {
        let n = self.nodo(id).ok_or(Error::NoExiste)?;
        n.volumen = v.map(|x| x.clamp(0, 4 * UNO_VOL));
        Ok(())
    }

    pub fn silencio(&mut self, id: Id, si: bool) -> Result<(), Error> {
        self.nodo(id).ok_or(Error::NoExiste)?.silencio = si;
        Ok(())
    }

    /// Un efecto encendido o puenteado (con un fundido cruzado de un cuanto entre lo procesado y lo que entra).
    pub fn activar(&mut self, id: Id, si: bool) -> Result<(), Error> {
        match &mut self.nodo(id).ok_or(Error::NoExiste)?.cuerpo {
            Cuerpo::Efecto { activo, .. } => {
                *activo = si;
                Ok(())
            }
            _ => Err(Error::Puerto),
        }
    }

    pub fn activo(&self, id: Id) -> Option<bool> {
        match &self.nodos.iter().find(|n| n.id == id)?.cuerpo {
            Cuerpo::Efecto { activo, .. } => Some(*activo),
            _ => None,
        }
    }

    /// El efecto de un nodo, para cambiar o leer sus parámetros.
    pub fn efecto_de(&mut self, id: Id) -> Option<&mut dyn Efecto> {
        match &mut self.nodo(id)?.cuerpo {
            Cuerpo::Efecto { efecto, .. } => Some(efecto.as_mut()),
            _ => None,
        }
    }

    /// Añade muestras a un flujo; devuelve los bytes que tomó (hasta un segundo de cola).
    pub fn escribir(&mut self, id: Id, bytes: &[u8]) -> usize {
        let Some(n) = self.nodo(id) else { return 0 };
        if n.fase != Fase::Suena {
            return 0;
        }
        let Cuerpo::Fuente(f) = &mut n.cuerpo else { return 0 };
        let caben = cabida(&f.formato).saturating_sub(f.cola.len()) * f.formato.bytes_por_cuadro();
        let usados = convertir(&f.formato, &bytes[..bytes.len().min(caben)], &mut f.cola);
        if usados > 0 {
            f.empezado = true;
        }
        usados
    }

    /// Cuadros que esperan en la cola de un flujo.
    pub fn pendientes(&self, id: Id) -> Option<usize> {
        match &self.nodos.iter().find(|n| n.id == id)?.cuerpo {
            Cuerpo::Fuente(f) => Some(f.cola.len()),
            _ => None,
        }
    }

    pub fn faltas(&self, id: Id) -> Option<u64> {
        match &self.nodos.iter().find(|n| n.id == id)?.cuerpo {
            Cuerpo::Fuente(f) => Some(f.faltas),
            _ => None,
        }
    }

    /// Reajusta el ritmo de un flujo, en partes por millón (de -10.000 a 10.000).
    pub fn ajustar(&mut self, id: Id, ppm: i32) -> bool {
        let Some(n) = self.nodo(id) else { return false };
        let Cuerpo::Fuente(f) = &mut n.cuerpo else { return false };
        let ppm = ppm.clamp(-10_000, 10_000);
        f.remuestreo.paso = ((f.remuestreo.nominal as u128 * (1_000_000 + ppm as i64) as u128) / 1_000_000) as u64;
        true
    }

    /// La medida de lo que salió del nodo desde la última lectura (y la pone a cero).
    pub fn medida(&mut self, id: Id) -> Option<Medida> {
        Some(self.nodo(id)?.medidor.leer())
    }

    /// Cuadros que esperan en la cola de una salida.
    pub fn listos(&self, salida: Id) -> Option<usize> {
        match &self.nodos.iter().find(|n| n.id == salida)?.cuerpo {
            Cuerpo::Destino(d) => Some(d.cola.len()),
            _ => None,
        }
    }

    pub fn periodo(&self, salida: Id) -> Option<usize> {
        match &self.nodos.iter().find(|n| n.id == salida)?.cuerpo {
            Cuerpo::Destino(d) => Some(d.periodo),
            _ => None,
        }
    }

    pub fn desbordes(&self, salida: Id) -> Option<u64> {
        match &self.nodos.iter().find(|n| n.id == salida)?.cuerpo {
            Cuerpo::Destino(d) => Some(d.desbordes),
            _ => None,
        }
    }

    /// Lleva a `dst` lo que haya en la cola de la salida (hasta llenarlo); devuelve los cuadros.
    pub fn sacar(&mut self, salida: Id, dst: &mut [Cuadro]) -> usize {
        let Some(n) = self.nodo(salida) else { return 0 };
        let Cuerpo::Destino(d) = &mut n.cuerpo else { return 0 };
        let k = dst.len().min(d.cola.len());
        for (i, c) in d.cola.drain(..k).enumerate() {
            dst[i] = c;
        }
        k
    }

    /// Procesa la salida hasta tener al menos `cuadros` en su cola, en cuantos (todo el grafo corre en cada uno).
    pub fn llenar(&mut self, salida: Id, cuadros: usize) {
        let mut vueltas = 0;
        while self.listos(salida).is_some_and(|l| l < cuadros) && vueltas < 1024 {
            self.procesar();
            vueltas += 1;
        }
    }

    /// El orden topológico (Kahn) y, para cada nodo, de dónde le llega lo suyo.
    fn planear(&mut self) {
        let n = self.nodos.len();
        let mut grado = alloc::vec![0usize; n];
        let sitio = |nodos: &[Nodo], id: Id| nodos.iter().position(|x| x.id == id);
        let aristas: Vec<(usize, usize, usize)> = self
            .conexiones
            .iter()
            .enumerate()
            .filter_map(|(k, c)| Some((k, sitio(&self.nodos, c.de)?, sitio(&self.nodos, c.a)?)))
            .collect();
        for &(_, _, j) in &aristas {
            grado[j] += 1;
        }
        let mut listos: VecDeque<usize> = (0..n).filter(|&i| grado[i] == 0).collect();
        self.plan.clear();
        while let Some(i) = listos.pop_front() {
            let entradas = aristas.iter().filter(|&&(_, _, j)| j == i).map(|&(k, de, _)| (k, de)).collect();
            self.plan.push(Paso { nodo: i, entradas });
            for &(_, de, j) in &aristas {
                if de == i {
                    grado[j] -= 1;
                    if grado[j] == 0 {
                        listos.push_back(j);
                    }
                }
            }
        }
        // `conectar` no deja hacer ciclos: todos los nodos están en el plan.
        debug_assert_eq!(self.plan.len(), n);
        self.sucio = false;
    }

    /// Un cuanto de todo el grafo.
    pub fn procesar(&mut self) {
        if self.sucio {
            self.planear();
        }
        let q = self.cuanto;
        let fundido = self.fundido() as i64;
        let tope_cola = self.frecuencia as usize;
        let mut entrada = core::mem::take(&mut self.entrada);
        for p in 0..self.plan.len() {
            let i = self.plan[p].nodo;
            // Lo que le entra: la suma de lo que sale de sus orígenes, cada uno por la rampa de su conexión.
            entrada.clear();
            entrada.resize(q, [0; 2]);
            let qn = q as i64;
            let mut entra_algo = false;
            for &(k, de) in &self.plan[p].entradas {
                let c = &self.conexiones[k];
                let (a, b) = (c.actual, c.pedida);
                let src = &self.nodos[de].buf;
                if (a == 0 && b == 0) || self.nodos[de].callado {
                    continue;
                }
                entra_algo = true;
                for (t, (e, s)) in entrada.iter_mut().zip(src.iter()).enumerate() {
                    let g = if a == b { a } else { a + (b - a) * (t as i64 + 1) / qn };
                    if g == UNO_VOL {
                        e[0] = e[0].saturating_add(s[0]);
                        e[1] = e[1].saturating_add(s[1]);
                    } else {
                        e[0] = e[0].saturating_add(((s[0] as i64 * g) >> 16) as i32);
                        e[1] = e[1].saturating_add(((s[1] as i64 * g) >> 16) as i32);
                    }
                }
            }
            let nodo = &mut self.nodos[i];
            let mut buf = core::mem::take(&mut nodo.buf);
            buf.clear();
            buf.resize(q, [0; 2]);
            match &mut nodo.cuerpo {
                Cuerpo::Fuente(f) => {
                    let mut falto = false;
                    for s in buf.iter_mut() {
                        match f.remuestreo.siguiente(&mut f.cola) {
                            Some(c) => *s = c,
                            None => falto = true,
                        }
                    }
                    if falto && f.empezado && nodo.fase == Fase::Suena {
                        f.faltas += 1;
                    }
                }
                Cuerpo::Mezcla | Cuerpo::Destino(_) => buf.copy_from_slice(&entrada),
                Cuerpo::Efecto { efecto, activo, mezcla, seco } => {
                    let objetivo = if *activo { UNO_VOL } else { 0 };
                    if !entra_algo && nodo.callado && *mezcla == objetivo {
                        // En reposo: ceros (ya lo son).
                    } else if *mezcla == 0 && objetivo == 0 {
                        buf.copy_from_slice(&entrada);
                    } else {
                        buf.copy_from_slice(&entrada);
                        efecto.procesar(&mut buf);
                        if *mezcla != UNO_VOL || objetivo != UNO_VOL {
                            // Fundido cruzado entre lo procesado y lo que entra.
                            seco.clear();
                            seco.extend_from_slice(&entrada);
                            let a = *mezcla;
                            for (t, (o, s)) in buf.iter_mut().zip(seco.iter()).enumerate() {
                                let g = a + (objetivo - a) * (t as i64 + 1) / qn;
                                for k in 0..2 {
                                    o[k] = (((o[k] as i64 * g) + (s[k] as i64 * (UNO_VOL - g))) >> 16) as i32;
                                }
                            }
                        }
                        *mezcla = objetivo;
                    }
                }
            }
            // La ganancia del nodo (y su fundido de salida), en rampa.
            let pedida = nodo.pedida();
            let (a, b) = (nodo.actual, pedida);
            let saliendo = nodo.fase != Fase::Suena;
            if a != [UNO_VOL; 2] || b != [UNO_VOL; 2] || saliendo {
                for (t, s) in buf.iter_mut().enumerate() {
                    let mut g = [0, 1].map(|c| if a[c] == b[c] { a[c] } else { a[c] + (b[c] - a[c]) * (t as i64 + 1) / qn });
                    if let Fase::Saliendo(queda) = nodo.fase {
                        let k = queda.saturating_sub(1) as i64;
                        g = g.map(|x| x * k / fundido);
                        nodo.fase = Fase::Saliendo(queda.saturating_sub(1));
                    }
                    s[0] = ((s[0] as i64 * g[0]) >> 16).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                    s[1] = ((s[1] as i64 * g[1]) >> 16).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                }
            }
            nodo.actual = b;
            nodo.callado = buf.iter().all(|c| *c == [0, 0]);
            // Los vúmetros: de lo que se enseña (flujos, mezclas y salidas), no de los efectos.
            if !nodo.callado && !matches!(nodo.cuerpo, Cuerpo::Efecto { .. }) {
                nodo.medidor.medir(&buf);
            }
            if let Cuerpo::Destino(d) = &mut nodo.cuerpo {
                d.cola.extend(buf.iter().copied());
                if d.cola.len() > tope_cola {
                    let sobra = d.cola.len() - tope_cola;
                    d.cola.drain(..sobra);
                    d.desbordes += sobra as u64;
                }
            }
            nodo.buf = buf;
        }
        self.entrada = entrada;
        // Las rampas de las conexiones llegan a lo pedido; se quitan las que acabaron de salir y los nodos que
        // acabaron su fundido (con sus conexiones).
        for c in self.conexiones.iter_mut() {
            c.actual = c.pedida;
        }
        let fuera: Vec<Id> = self.nodos.iter().filter(|n| n.fase == Fase::Saliendo(0)).map(|n| n.id).collect();
        let antes = (self.conexiones.len(), self.nodos.len());
        self.conexiones.retain(|c| c.pedida != 0 && !fuera.contains(&c.de) && !fuera.contains(&c.a));
        self.nodos.retain(|n| !fuera.contains(&n.id));
        if antes != (self.conexiones.len(), self.nodos.len()) {
            self.sucio = true;
        }
    }
}

#[cfg(test)]
#[path = "pruebas_grafo.rs"]
mod pruebas;

