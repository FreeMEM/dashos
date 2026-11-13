//! Trasfondo nativo: lo que miden el panel del fondo, la barra y el centro de control, sin dibujo. Las cuentas del uso
//! de CPU (por núcleo y por proceso) y de la red a partir de contadores que solo suben, las entradas de `MONITOR` de
//! `sesiones` y los ajustes del panel (`Prefs/monitor`).
//!
//! Los contadores pueden dar sorpresas, y ninguna tumba nada ni da un número imposible:
//! - un contador que baja (dio la vuelta, o el servicio volvió a empezar): esa medida no cuenta, se empieza de nuevo;
//! - un núcleo que aparece o desaparece: el que aparece empieza sin historia; el que se va, se olvida;
//! - un proceso que nace y muere entre dos medidas: no sale (no hay dos medidas suyas); uno nuevo sale con 0 % hasta la
//!   siguiente;
//! - medidas demasiado seguidas (menos de `MIN_INTERVALO_US`): no se calcula nada (el uso sería ruido).
//!
//! Se prueba en el PC con `rustc --edition 2024 --test usuario/src/monitor.rs`.

#[cfg(test)]
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Las medidas que se guardan para las gráficas (como Trasfondo: 60).
pub const HISTORIAL: usize = 60;
/// Núcleos que se miran como mucho (los del kernel).
pub const MAX_NUCLEOS: usize = 4;
/// Por debajo de esto entre dos medidas, no se calcula el uso.
pub const MIN_INTERVALO_US: u64 = 50_000;
/// Los procesos del bloque del panel (como Trasfondo).
pub const LOS_QUE_MAS: usize = 6;

/// Uso en décimas de punto (0 a 1000).
pub type Decimas = u32;

/// Lo que se ve de un proceso (o de una suma) tras `MONITOR`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entrada {
    pub marca: u64,
    pub cpu_us: u64,
    pub memoria: u64,
    pub uid: u32,
    pub gid: u32,
    pub clase: u32,
    pub cuantos: u32,
    pub nombre: String,
}

/// Las clases (como `protocolo::sesiones::M_*`, repetidas para probar en el PC sin la biblioteca).
pub const M_PROPIO: u32 = 0;
pub const M_CAJA_AMIGA: u32 = 1;
pub const M_CAJA_LINUX: u32 = 2;
pub const M_SISTEMA: u32 = 3;
pub const M_OTROS: u32 = 4;
pub const ENTRADA: usize = 56;
pub const POR_PAGINA: usize = 4;

impl Entrada {
    /// El nombre que se enseña: el del proceso, o el de la suma.
    pub fn titulo(&self) -> String {
        match self.clase {
            M_SISTEMA => alloc::format!("Sistema ({})", self.cuantos),
            M_OTROS => alloc::format!("Otros usuarios ({})", self.cuantos),
            M_CAJA_AMIGA => String::from("Caja Amiga"),
            M_CAJA_LINUX => String::from("Caja Linux"),
            _ => self.nombre.clone(),
        }
    }

    /// La pastilla de la caja, si es una.
    pub fn pastilla(&self) -> Option<&'static str> {
        match self.clase {
            M_CAJA_AMIGA => Some("68K"),
            M_CAJA_LINUX => Some("LINUX"),
            _ => None,
        }
    }

    /// Una clave que no se repite entre medidas: la marca, o la clase en una suma.
    pub fn clave(&self) -> u64 {
        if self.marca != 0 { self.marca } else { u64::MAX - self.clase as u64 }
    }
}

/// Una página de la respuesta de `MONITOR`: cuántas hay y las que van. `None` si no tiene forma (lo que no cuadra no
/// se lee: un nombre que no es UTF-8 sale con "?").
pub fn leer_pagina(r: &[u8]) -> Option<(usize, Vec<Entrada>)> {
    if r.len() < 8 {
        return None;
    }
    let u32_en = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let u64_en = |b: &[u8], i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
    let van = u32_en(r, 0) as usize;
    let total = u32_en(r, 4) as usize;
    if van > POR_PAGINA || r.len() < 8 + van * ENTRADA {
        return None;
    }
    let mut v = Vec::new();
    for k in 0..van {
        let e = &r[8 + k * ENTRADA..8 + (k + 1) * ENTRADA];
        let n = &e[40..56];
        let fin = n.iter().position(|&c| c == 0).unwrap_or(16);
        let nombre: String = core::str::from_utf8(&n[..fin]).unwrap_or("?").chars().filter(|c| !c.is_control()).collect();
        v.push(Entrada {
            marca: u64_en(e, 0),
            cpu_us: u64_en(e, 8),
            memoria: u64_en(e, 16),
            uid: u32_en(e, 24),
            gid: u32_en(e, 28),
            clase: u32_en(e, 32).min(M_OTROS),
            cuantos: u32_en(e, 36),
            nombre,
        });
    }
    Some((total, v))
}

/// Escribe una entrada en `e` (`ENTRADA` bytes), como la manda `sesiones`.
pub fn escribir_entrada(e: &mut [u8], x: &Entrada) {
    e[0..8].copy_from_slice(&x.marca.to_le_bytes());
    e[8..16].copy_from_slice(&x.cpu_us.to_le_bytes());
    e[16..24].copy_from_slice(&x.memoria.to_le_bytes());
    e[24..28].copy_from_slice(&x.uid.to_le_bytes());
    e[28..32].copy_from_slice(&x.gid.to_le_bytes());
    e[32..36].copy_from_slice(&x.clase.to_le_bytes());
    e[36..40].copy_from_slice(&x.cuantos.to_le_bytes());
    e[40..56].fill(0);
    let n = x.nombre.len().min(16);
    e[40..40 + n].copy_from_slice(&x.nombre.as_bytes()[..n]);
}

/// El uso entre dos medidas de un contador ocioso: `(ocioso_us, ahora_us)` antes y después. `None` si no se puede
/// saber (contador que baja, tiempo que no avanza o muy poco).
pub fn uso_de(antes: (u64, u64), despues: (u64, u64)) -> Option<Decimas> {
    let dt = despues.1.checked_sub(antes.1)?;
    let di = despues.0.checked_sub(antes.0)?;
    if dt < MIN_INTERVALO_US {
        return None;
    }
    Some((1000 - (di.min(dt) as u128 * 1000 / dt as u128) as u64) as Decimas)
}

/// Un anillo de las últimas `HISTORIAL` medidas.
#[derive(Clone, Debug, Default)]
pub struct Historial {
    pub v: Vec<Decimas>,
}

impl Historial {
    pub fn meter(&mut self, x: Decimas) {
        if self.v.len() == HISTORIAL {
            self.v.remove(0);
        }
        self.v.push(x.min(1000));
    }

    pub fn ultimo(&self) -> Option<Decimas> {
        self.v.last().copied()
    }
}

/// Un proceso con su uso de ahora.
#[derive(Clone, Debug)]
pub struct Visto {
    pub e: Entrada,
    /// Del total de la máquina (todos los núcleos), en décimas.
    pub cpu: Decimas,
}

/// Lo que se lleva entre medidas.
#[derive(Default)]
pub struct Medidor {
    /// Por núcleo: la última medida (ocioso, ahora) y su historial.
    nucleos: [Option<(u64, u64)>; MAX_NUCLEOS],
    pub por_nucleo: [Historial; MAX_NUCLEOS],
    pub uso_nucleo: [Option<Decimas>; MAX_NUCLEOS],
    /// El total de la máquina (media de los núcleos en marcha).
    pub total: Historial,
    /// Por proceso: CPU y momento de la última medida.
    procesos: BTreeMap<u64, (u64, u64)>,
    pub vistos: Vec<Visto>,
    /// La red: bytes que entran y salen y el momento; lo de ahora por segundo; y sus historiales (en décimas del máximo
    /// visto, para la gráfica).
    red: Option<(u64, u64, u64)>,
    pub entra_s: Option<u64>,
    pub sale_s: Option<u64>,
    pub red_hist: Vec<(u64, u64)>,
    /// La memoria: en uso y total, en bytes, y su historial (décimas).
    pub memoria: Option<(u64, u64)>,
    pub memoria_hist: Historial,
    /// Cuántos núcleos están en marcha.
    pub en_marcha: u32,
}

impl Medidor {
    /// Los núcleos: `presentes` (un bit por núcleo) y para cada uno, sus µs ociosos (`None` si no se pudo leer).
    pub fn nucleos(&mut self, presentes: u64, ociosos: &[Option<u64>; MAX_NUCLEOS], ahora: u64) {
        let mut suma = 0u64;
        let mut cuantos = 0u64;
        self.en_marcha = 0;
        for n in 0..MAX_NUCLEOS {
            let esta = presentes & (1 << n) != 0;
            match (esta, ociosos[n]) {
                (true, Some(o)) => {
                    self.en_marcha += 1;
                    let uso = self.nucleos[n].and_then(|a| uso_de(a, (o, ahora)));
                    // Un contador que baja: se empieza de nuevo con esta medida.
                    self.nucleos[n] = Some((o, ahora));
                    self.uso_nucleo[n] = uso;
                    if let Some(u) = uso {
                        self.por_nucleo[n].meter(u);
                        suma += u as u64;
                        cuantos += 1;
                    }
                }
                _ => {
                    // No está (o no se lee): se olvida, y su historial también.
                    self.nucleos[n] = None;
                    self.uso_nucleo[n] = None;
                    self.por_nucleo[n] = Historial::default();
                }
            }
        }
        if cuantos > 0 {
            self.total.meter((suma / cuantos) as Decimas);
        }
    }

    /// Los procesos de esta medida (todas las páginas de `MONITOR`). `ahora` en µs. Calcula el uso de cada uno respecto
    /// a la máquina entera (`en_marcha` núcleos) y olvida a los que ya no están.
    pub fn procesos(&mut self, entradas: Vec<Entrada>, ahora: u64) {
        let nucleos = self.en_marcha.max(1) as u128;
        let mut nuevos = BTreeMap::new();
        let mut vistos = Vec::new();
        for e in entradas {
            let clave = e.clave();
            if nuevos.contains_key(&clave) {
                continue;
            }
            let cpu = match self.procesos.get(&clave) {
                Some(&(c0, t0)) => match (e.cpu_us.checked_sub(c0), ahora.checked_sub(t0)) {
                    (Some(dc), Some(dt)) if dt >= MIN_INTERVALO_US => (dc as u128 * 1000 / (dt as u128 * nucleos)).min(1000) as Decimas,
                    _ => 0,
                },
                None => 0,
            };
            nuevos.insert(clave, (e.cpu_us, ahora));
            vistos.push(Visto { e, cpu });
        }
        self.procesos = nuevos;
        self.vistos = vistos;
    }

    /// La red: bytes que entraron y salieron desde que arrancó el servicio (`None` sin servicio).
    pub fn red(&mut self, bytes: Option<(u64, u64)>, ahora: u64) {
        let Some((rx, tx)) = bytes else {
            self.red = None;
            self.entra_s = None;
            self.sale_s = None;
            return;
        };
        if let Some((rx0, tx0, t0)) = self.red
            && let (Some(dr), Some(ds), Some(dt)) = (rx.checked_sub(rx0), tx.checked_sub(tx0), ahora.checked_sub(t0))
            && dt >= MIN_INTERVALO_US
        {
            let e = (dr as u128 * 1_000_000 / dt as u128) as u64;
            let s = (ds as u128 * 1_000_000 / dt as u128) as u64;
            self.entra_s = Some(e);
            self.sale_s = Some(s);
            if self.red_hist.len() == HISTORIAL {
                self.red_hist.remove(0);
            }
            self.red_hist.push((e, s));
        } else if self.red.is_some_and(|(rx0, tx0, _)| rx < rx0 || tx < tx0) {
            // Dio la vuelta o volvió a empezar: sin número esta vez.
            self.entra_s = None;
            self.sale_s = None;
        }
        self.red = Some((rx, tx, ahora));
    }

    /// La memoria: páginas en total y libres.
    pub fn memoria(&mut self, total_paginas: u64, libres: u64) {
        if total_paginas == 0 || libres > total_paginas {
            return;
        }
        let total = total_paginas * 4096;
        let usada = (total_paginas - libres) * 4096;
        self.memoria = Some((usada, total));
        self.memoria_hist.meter((usada as u128 * 1000 / total as u128) as Decimas);
    }

    /// Los que más CPU gastan (y, a igualdad, más memoria), hasta `n`.
    pub fn los_que_mas(&self, n: usize) -> Vec<&Visto> {
        let mut v: Vec<&Visto> = self.vistos.iter().collect();
        v.sort_by(|a, b| b.cpu.cmp(&a.cpu).then(b.e.memoria.cmp(&a.e.memoria)).then(a.e.clave().cmp(&b.e.clave())));
        v.truncate(n);
        v
    }
}

/// Décimas como "12,3 %".
pub fn por_ciento(d: Decimas) -> String {
    alloc::format!("{},{} %", d / 10, d % 10)
}

/// Bytes como "512 KB", "12,3 MB" o "1,2 GB".
pub fn bytes(b: u64) -> String {
    const KB: u64 = 1 << 10;
    const MB: u64 = 1 << 20;
    const GB: u64 = 1 << 30;
    if b < KB {
        alloc::format!("{} B", b)
    } else if b < MB {
        alloc::format!("{} KB", b / KB)
    } else if b < GB {
        let d = b * 10 / MB;
        alloc::format!("{},{} MB", d / 10, d % 10)
    } else {
        let d = b / (GB / 10);
        alloc::format!("{},{} GB", d / 10, d % 10)
    }
}

/// Por segundo: "1,2 KB/s".
pub fn caudal(b: u64) -> String {
    alloc::format!("{}/s", bytes(b))
}

/// La esquina del panel en el fondo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Esquina {
    ArribaIzquierda,
    ArribaDerecha,
    AbajoIzquierda,
    AbajoDerecha,
}

pub const ESQUINAS: [(Esquina, &str, &str); 4] = [
    (Esquina::ArribaIzquierda, "arriba-izquierda", "arriba a la izquierda"),
    (Esquina::ArribaDerecha, "arriba-derecha", "arriba a la derecha"),
    (Esquina::AbajoIzquierda, "abajo-izquierda", "abajo a la izquierda"),
    (Esquina::AbajoDerecha, "abajo-derecha", "abajo a la derecha"),
];

/// Los bloques del panel, en su orden.
pub const BLOQUES: [(&str, &str); 6] = [
    ("reloj", "Hora y fecha"),
    ("cpu", "CPU por núcleo"),
    ("memoria", "Memoria"),
    ("procesos", "Los que más gastan"),
    ("discos", "Discos"),
    ("red", "Red"),
];

/// Los intervalos que se ofrecen, en segundos (como Trasfondo).
pub const INTERVALOS: [u32; 4] = [1, 2, 5, 10];

/// Los ajustes del panel (`Prefs/monitor`), como TrasfondoPrefs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ajustes {
    pub mostrar: bool,
    pub esquina: Esquina,
    /// Un bit por bloque de `BLOQUES`.
    pub bloques: u32,
    pub intervalo: u32,
    pub graficas: bool,
}

impl Ajustes {
    pub const DE_SERIE: Ajustes = Ajustes { mostrar: true, esquina: Esquina::AbajoIzquierda, bloques: 0b11_1111, intervalo: 2, graficas: true };

    pub fn bloque(&self, nombre: &str) -> bool {
        BLOQUES.iter().position(|b| b.0 == nombre).is_some_and(|k| self.bloques & (1 << k) != 0)
    }

    /// El texto del fichero: una línea por ajuste.
    pub fn escribir(&self) -> String {
        let bloques: Vec<&str> = BLOQUES.iter().enumerate().filter(|(k, _)| self.bloques & (1 << k) != 0).map(|(_, b)| b.0).collect();
        let esquina = ESQUINAS.iter().find(|e| e.0 == self.esquina).map_or("abajo-izquierda", |e| e.1);
        alloc::format!(
            "mostrar {}\nesquina {}\nbloques {}\nintervalo {}\ngraficas {}\n",
            if self.mostrar { "si" } else { "no" },
            esquina,
            if bloques.is_empty() { String::from("ninguno") } else { bloques.join(" ") },
            self.intervalo,
            if self.graficas { "si" } else { "no" }
        )
    }

    /// Lee el fichero. Lo que falta, de serie; una línea que no se entiende, `Err` (y quien lee se queda con los de
    /// serie).
    pub fn leer(texto: &str) -> Result<Ajustes, &'static str> {
        if texto.len() > 512 {
            return Err("demasiado largo");
        }
        let mut a = Ajustes::DE_SERIE;
        for linea in texto.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (clave, valor) = linea.split_once(' ').unwrap_or((linea, ""));
            let si_no = |v: &str| match v {
                "si" => Ok(true),
                "no" => Ok(false),
                _ => Err("un sí o no que no se entiende"),
            };
            match clave {
                "mostrar" => a.mostrar = si_no(valor)?,
                "graficas" => a.graficas = si_no(valor)?,
                "esquina" => a.esquina = ESQUINAS.iter().find(|e| e.1 == valor).ok_or("esquina que no existe")?.0,
                "intervalo" => {
                    let s: u32 = valor.parse().map_err(|_| "intervalo que no es un número")?;
                    if !INTERVALOS.contains(&s) {
                        return Err("intervalo que no se ofrece");
                    }
                    a.intervalo = s;
                }
                "bloques" => {
                    a.bloques = 0;
                    for b in valor.split_whitespace().filter(|b| *b != "ninguno") {
                        let k = BLOQUES.iter().position(|x| x.0 == b).ok_or("bloque que no existe")?;
                        a.bloques |= 1 << k;
                    }
                }
                _ => return Err("línea que no se entiende"),
            }
        }
        Ok(a)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn entrada(marca: u64, cpu_us: u64, clase: u32) -> Entrada {
        Entrada { marca, cpu_us, memoria: 1 << 20, uid: 1000, gid: 1000, clase, cuantos: 1, nombre: alloc::format!("p{}", marca) }
    }

    #[test]
    fn uso_normal_y_raro() {
        // 1 s, 250 ms ociosos: 75 %.
        assert_eq!(uso_de((0, 0), (250_000, 1_000_000)), Some(750));
        // Ocioso todo el rato, o más (redondeos del kernel): 0 %.
        assert_eq!(uso_de((0, 0), (1_200_000, 1_000_000)), Some(0));
        // Contador que baja (dio la vuelta o el núcleo volvió a empezar): nada.
        assert_eq!(uso_de((500, 0), (100, 1_000_000)), None);
        // Tiempo que no avanza, o que vuelve atrás, o muy poco.
        assert_eq!(uso_de((0, 1000), (0, 1000)), None);
        assert_eq!(uso_de((0, 1000), (0, 10)), None);
        assert_eq!(uso_de((0, 0), (0, 10_000)), None);
        // Números enormes: sin desbordar.
        assert_eq!(uso_de((u64::MAX - 10, u64::MAX - 2_000_000), (u64::MAX - 5, u64::MAX)), Some(1000));
    }

    #[test]
    fn nucleos_que_aparecen_y_desaparecen() {
        let mut m = Medidor::default();
        m.nucleos(0b11, &[Some(0), Some(0), None, None], 0);
        assert_eq!(m.uso_nucleo, [None; 4]);
        m.nucleos(0b11, &[Some(500_000), Some(0), None, None], 1_000_000);
        assert_eq!(m.uso_nucleo[0], Some(500));
        assert_eq!(m.uso_nucleo[1], Some(1000));
        assert_eq!(m.total.ultimo(), Some(750));
        // Aparece el 2 (sin historia) y se va el 1 (se olvida).
        m.nucleos(0b101, &[Some(1_000_000), Some(5), Some(42), None], 2_000_000);
        assert_eq!(m.uso_nucleo[1], None);
        assert_eq!(m.uso_nucleo[2], None);
        assert!(m.por_nucleo[1].v.is_empty());
        assert_eq!(m.uso_nucleo[0], Some(500));
        assert_eq!(m.en_marcha, 2);
        // El 0 da la vuelta: esa vez no cuenta, la siguiente sí.
        m.nucleos(0b101, &[Some(3), Some(0), Some(42), None], 3_000_000);
        assert_eq!(m.uso_nucleo[0], None);
        m.nucleos(0b101, &[Some(500_003), Some(0), Some(1_000_042), None], 4_000_000);
        assert_eq!(m.uso_nucleo[0], Some(500));
        // Un bit de un núcleo que no se lee: como si no estuviera.
        m.nucleos(0b1, &[None, None, None, None], 5_000_000);
        assert_eq!(m.en_marcha, 0);
    }

    #[test]
    fn procesos_que_nacen_y_mueren() {
        let mut m = Medidor::default();
        m.en_marcha = 2;
        m.procesos(alloc::vec![entrada(1, 0, M_PROPIO), entrada(2, 100, M_PROPIO)], 0);
        assert!(m.vistos.iter().all(|v| v.cpu == 0));
        // 1 s después: el 1 gastó 1 s de CPU (un núcleo entero de dos: 50 %); el 2 murió y nació el 3.
        m.procesos(alloc::vec![entrada(1, 1_000_000, M_PROPIO), entrada(3, 900_000, M_PROPIO)], 1_000_000);
        assert_eq!(m.vistos.len(), 2);
        assert_eq!(m.vistos[0].cpu, 500);
        // El 3 nació entre medidas: 0 hasta la siguiente (no se sabe desde cuándo).
        assert_eq!(m.vistos[1].cpu, 0);
        // Un contador que baja (la marca se reusó, o lo que sea): 0, sin pánico.
        m.procesos(alloc::vec![entrada(1, 5, M_PROPIO)], 2_000_000);
        assert_eq!(m.vistos[0].cpu, 0);
        // Más de lo posible (núcleos que desaparecieron): como mucho 100 %.
        m.en_marcha = 1;
        m.procesos(alloc::vec![entrada(1, 50_000_000, M_PROPIO)], 3_000_000);
        assert_eq!(m.vistos[0].cpu, 1000);
        // Las sumas se siguen por su clase, y una repetida no sale dos veces.
        m.procesos(alloc::vec![entrada(0, 0, M_SISTEMA), entrada(0, 0, M_SISTEMA), entrada(0, 0, M_OTROS)], 4_000_000);
        assert_eq!(m.vistos.len(), 2);
        // Medidas demasiado seguidas: 0, no ruido.
        m.procesos(alloc::vec![entrada(0, 1_000, M_SISTEMA)], 4_000_010);
        assert_eq!(m.vistos[0].cpu, 0);
    }

    #[test]
    fn los_que_mas() {
        let mut m = Medidor::default();
        m.en_marcha = 1;
        let es: Vec<Entrada> = (1..=8).map(|k| entrada(k, 0, M_PROPIO)).collect();
        m.procesos(es, 0);
        let es: Vec<Entrada> = (1..=8).map(|k| entrada(k, k * 10_000, M_PROPIO)).collect();
        m.procesos(es, 1_000_000);
        let v = m.los_que_mas(LOS_QUE_MAS);
        assert_eq!(v.len(), 6);
        assert_eq!(v[0].e.marca, 8);
        assert_eq!(v[5].e.marca, 3);
    }

    #[test]
    fn red_por_segundo() {
        let mut m = Medidor::default();
        m.red(Some((1000, 500)), 0);
        assert_eq!(m.entra_s, None);
        m.red(Some((3000, 1500)), 2_000_000);
        assert_eq!((m.entra_s, m.sale_s), (Some(1000), Some(500)));
        // El servicio volvió a empezar: sin número esta vez, y después sí.
        m.red(Some((10, 10)), 3_000_000);
        assert_eq!(m.entra_s, None);
        m.red(Some((1010, 10)), 4_000_000);
        assert_eq!(m.entra_s, Some(1000));
        m.red(None, 5_000_000);
        assert_eq!(m.entra_s, None);
    }

    #[test]
    fn paginas_de_sesiones() {
        let mut r = [0u8; 8 + 2 * ENTRADA];
        r[0..4].copy_from_slice(&2u32.to_le_bytes());
        r[4..8].copy_from_slice(&9u32.to_le_bytes());
        let mut e = entrada(7, 123, M_CAJA_AMIGA);
        e.nombre = "caja-amiga".into();
        escribir_entrada(&mut r[8..8 + ENTRADA], &e);
        escribir_entrada(&mut r[8 + ENTRADA..], &entrada(0, 5, M_SISTEMA));
        let (total, v) = leer_pagina(&r).unwrap();
        assert_eq!(total, 9);
        assert_eq!(v[0], e);
        assert_eq!(v[0].pastilla(), Some("68K"));
        assert_eq!(v[1].titulo(), "Sistema (1)");
        // Cortas, o que dicen más de las que traen, o una clase rara.
        assert!(leer_pagina(&r[..20]).is_none());
        assert!(leer_pagina(&[0; 4]).is_none());
        let mut mal = r;
        mal[0..4].copy_from_slice(&200u32.to_le_bytes());
        assert!(leer_pagina(&mal).is_none());
        let mut rara = r;
        rara[8 + 32..8 + 36].copy_from_slice(&77u32.to_le_bytes());
        rara[8 + 40] = 0xff;
        let (_, v) = leer_pagina(&rara).unwrap();
        assert_eq!(v[0].clase, M_OTROS);
        assert_eq!(v[0].nombre, "?");
    }

    #[test]
    fn ajustes_ida_y_vuelta() {
        assert_eq!(Ajustes::leer("").unwrap(), Ajustes::DE_SERIE);
        let a = Ajustes { mostrar: false, esquina: Esquina::ArribaDerecha, bloques: 0b10_0110, intervalo: 5, graficas: false };
        assert_eq!(Ajustes::leer(&a.escribir()).unwrap(), a);
        let ninguno = Ajustes { bloques: 0, ..Ajustes::DE_SERIE };
        assert_eq!(Ajustes::leer(&ninguno.escribir()).unwrap(), ninguno);
        assert!(Ajustes::leer("intervalo 3").is_err());
        assert!(Ajustes::leer("intervalo -1").is_err());
        assert!(Ajustes::leer("esquina centro").is_err());
        assert!(Ajustes::leer("bloques cpu disquetera").is_err());
        assert!(Ajustes::leer("mostrar quizá").is_err());
        assert!(Ajustes::leer(&"x".repeat(600)).is_err());
        assert!(a.bloque("cpu") && !a.bloque("reloj") && !a.bloque("nada"));
    }

    #[test]
    fn textos() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(2048), "2 KB");
        assert_eq!(bytes(12 << 20 | 300 << 10), "12,2 MB");
        assert_eq!(bytes(3 << 30), "3,0 GB");
        assert_eq!(caudal(1536), "1 KB/s");
        assert_eq!(por_ciento(123), "12,3 %");
    }
}
