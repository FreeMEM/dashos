//! S27, oleada 3: los ajustes de los avisos (`ENV:Escritorio/avisos`, en texto: S36, regla 1) y lo que se calcula con
//! ellos: dónde salen los emergentes, cuánto duran, sus animaciones y si "no molestar" los calla ahora.
//!
//! Portado de DMS (DankMaterialShell v2.1.2, `Common/settings/SettingsSpec.js`, `Common/Theme.qml`,
//! `Modules/Notifications/NotificationMetrics.qml` y `Services/NotificationService.qml`; MIT, copyright (c) 2025 Avenge
//! Media LLC; aviso entero en `docs/licencias/DankMaterialShell-MIT.txt`). Las claves son las de DMS en castellano; los
//! valores de serie, los del escritorio del usuario en shiva (`notificationPopupPosition = -1`, arriba en el centro, y
//! `notificationAnimationDuration = 400`); lo demás, lo de serie de DMS.
//!
//! Lo que DMS no tiene y DashOS sí: un horario de "no molestar" (desde, hasta y días; DMS solo tiene plazos). El "no
//! molestar" a mano (con plazo o sin él) es estado de la sesión, como en DMS: lo lleva el servicio de avisos, no este
//! fichero.
//!
//! El centro de control lee el texto, lo pasa a bytes (`escribir`) y se los da al servicio de avisos, que se los da a la
//! barra (`leer`). Sin `alloc` ni nada de DashOS: se prueba en el PC con
//! `rustc --edition 2024 --test usuario/src/avisos_ajustes.rs`.

/// Dónde salen los emergentes (los números de `notificationPopupPosition` de DMS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Posicion {
    ArribaDerecha,
    ArribaIzquierda,
    ArribaCentro,
    AbajoDerecha,
    AbajoIzquierda,
    AbajoCentro,
}

/// Nombre en el fichero, número de DMS y posición.
const POSICIONES: [(&str, i32, Posicion); 6] = [
    ("arriba-derecha", 0, Posicion::ArribaDerecha),
    ("arriba-izquierda", 2, Posicion::ArribaIzquierda),
    ("arriba-centro", -1, Posicion::ArribaCentro),
    ("abajo-derecha", 3, Posicion::AbajoDerecha),
    ("abajo-izquierda", 1, Posicion::AbajoIzquierda),
    ("abajo-centro", 5, Posicion::AbajoCentro),
];

impl Posicion {
    pub fn nombre(self) -> &'static str {
        POSICIONES.iter().find(|p| p.2 == self).map_or("arriba-derecha", |p| p.0)
    }

    pub fn arriba(self) -> bool {
        matches!(self, Posicion::ArribaDerecha | Posicion::ArribaIzquierda | Posicion::ArribaCentro)
    }

    pub fn centrada(self) -> bool {
        matches!(self, Posicion::ArribaCentro | Posicion::AbajoCentro)
    }

    pub fn izquierda(self) -> bool {
        matches!(self, Posicion::ArribaIzquierda | Posicion::AbajoIzquierda)
    }

    fn numero(self) -> u8 {
        POSICIONES.iter().position(|p| p.2 == self).unwrap_or(0) as u8
    }
}

/// Qué enseña la pantalla de bloqueo (`lockNotifications.mode` de DMS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bloqueo {
    Nada,
    Contador,
    Nombres,
    Todo,
}

const BLOQUEOS: [(&str, Bloqueo); 4] = [("nada", Bloqueo::Nada), ("contador", Bloqueo::Contador), ("nombres", Bloqueo::Nombres), ("todo", Bloqueo::Todo)];

/// Un horario de "no molestar": desde y hasta (minutos del día, hora local) y los días en que empieza (bit 0 el lunes).
/// Si `desde` es mayor que `hasta`, cruza la medianoche; si son iguales, es el día entero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Horario {
    pub desde: u16,
    pub hasta: u16,
    pub dias: u8,
}

pub const TODOS_LOS_DIAS: u8 = 0x7f;
pub const DIAS: [&str; 7] = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];

impl Horario {
    /// Si `minuto` (del día, hora local) del día `dia` (0 lunes) cae dentro.
    pub fn dentro(&self, minuto: u32, dia: u32) -> bool {
        let (desde, hasta, dia) = (self.desde as u32, self.hasta as u32, dia % 7);
        let empieza = |d: u32| self.dias & (1 << (d % 7)) != 0;
        if desde == hasta {
            empieza(dia)
        } else if desde < hasta {
            empieza(dia) && minuto >= desde && minuto < hasta
        } else {
            // Cruza la medianoche: la noche que empieza hoy, o la que empezó ayer.
            (empieza(dia) && minuto >= desde) || (empieza(dia + 6) && minuto < hasta)
        }
    }
}

/// Los ajustes. `DE_SERIE`, lo del escritorio de shiva y lo de serie de DMS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ajustes {
    pub posicion: Posicion,
    /// La base de las animaciones (`notificationAnimationDuration`), de 0 (sin animación) a 800 ms.
    pub animacion_ms: u16,
    /// Cuánto se ve un emergente por urgencia (baja, normal, urgente), en ms; 0, hasta que se cierre.
    pub tiempos_ms: [u32; 3],
    /// La barra fina del tiempo que le queda (`notificationShowTimeoutBar`).
    pub barra_tiempo: bool,
    /// Los urgentes salen con "no molestar" (`notificationDndAllowCritical`).
    pub urgentes_pasan: bool,
    pub horario: Option<Horario>,
    /// Sin el texto en el emergente (`notificationPopupPrivacyMode`).
    pub privado: bool,
    /// Un clic en el cuerpo hace la acción por defecto (`notificationPopupBodyInvokesAction`).
    pub cuerpo_hace_accion: bool,
    pub compacto: bool,
    /// Guardar en el historial y cuántos días (0, siempre).
    pub historial: bool,
    pub dias_historial: u16,
    pub bloqueo: Bloqueo,
}

/// Las claves, en el orden en que se escriben.
pub const CLAVES: [&str; 16] = [
    "posicion",
    "animacion",
    "tiempo_baja",
    "tiempo_normal",
    "tiempo_urgente",
    "barra_tiempo",
    "urgentes_en_no_molestar",
    "no_molestar_desde",
    "no_molestar_hasta",
    "no_molestar_dias",
    "privado",
    "cuerpo_hace_accion",
    "compacto",
    "historial",
    "dias_historial",
    "bloqueo",
];

/// La cabecera del fichero (los comentarios), para quien lo escribe por primera vez.
pub const CABECERA: &str = "Los avisos (Ajustes > Avisos; la orden `Ajuste Escritorio/avisos.clave valor`). Este fichero manda.\n\
posicion: arriba-derecha, arriba-izquierda, arriba-centro, abajo-derecha, abajo-izquierda o abajo-centro.\n\
animacion: milisegundos de 0 (sin animación) a 800. tiempo_*: milisegundos que se ve un emergente; 0, hasta cerrarlo.\n\
no_molestar_desde y no_molestar_hasta: hh:mm (vacío, sin horario); no_molestar_dias: lun mar mié jue vie sáb dom, o todos.\n\
bloqueo: lo que se ve con la pantalla bloqueada: nada, contador, nombres o todo.";

/// Lo más que mide `escribir`.
pub const LARGO: usize = 32;
const VERSION: u8 = 1;

fn si_no(v: &str) -> Option<bool> {
    match v {
        "sí" | "si" | "Sí" | "Si" | "true" | "1" => Some(true),
        "no" | "No" | "false" | "0" => Some(false),
        _ => None,
    }
}

fn numero(v: &str, max: u32) -> Option<u32> {
    v.parse::<u32>().ok().filter(|&n| n <= max)
}

/// "hh:mm" a minutos del día.
pub fn hora_minutos(v: &str) -> Option<u16> {
    let (h, m) = v.split_once(':')?;
    let (h, m) = (h.trim().parse::<u16>().ok()?, m.trim().parse::<u16>().ok()?);
    (h < 24 && m < 60 && v.len() <= 5).then_some(h * 60 + m)
}

impl Ajustes {
    pub const DE_SERIE: Ajustes = Ajustes {
        posicion: Posicion::ArribaCentro,
        animacion_ms: 400,
        tiempos_ms: [5000, 5000, 0],
        barra_tiempo: false,
        urgentes_pasan: true,
        horario: None,
        privado: false,
        cuerpo_hace_accion: false,
        compacto: false,
        historial: true,
        dias_historial: 7,
        bloqueo: Bloqueo::Contador,
    };

    /// Pone una clave del fichero. `Err` con el porqué si la clave no existe o el valor no vale (y no cambia nada).
    /// La posición también se acepta con el número de DMS (para importar sus ajustes).
    pub fn poner(&mut self, clave: &str, valor: &str) -> Result<(), &'static str> {
        let v = valor.trim();
        // Sin `alloc`: la clave, en minúsculas en un búfer.
        let mut c = [0u8; 32];
        if clave.len() > c.len() {
            return Err("clave que no existe");
        }
        c[..clave.len()].copy_from_slice(clave.as_bytes());
        c[..clave.len()].make_ascii_lowercase();
        match &c[..clave.len()] {
            b"posicion" => {
                let p = POSICIONES.iter().find(|p| p.0.eq_ignore_ascii_case(v) || v.parse::<i32>().ok() == Some(p.1)).ok_or("posición que no existe")?;
                self.posicion = p.2;
            }
            b"animacion" => self.animacion_ms = numero(v, 800).ok_or("animación: de 0 a 800 ms")? as u16,
            b"tiempo_baja" | b"tiempo_normal" | b"tiempo_urgente" => {
                let i = match clave.as_bytes()[7] {
                    b'b' | b'B' => 0,
                    b'n' | b'N' => 1,
                    _ => 2,
                };
                self.tiempos_ms[i] = numero(v, 600_000).ok_or("tiempo: de 0 a 600000 ms")?;
            }
            b"barra_tiempo" => self.barra_tiempo = si_no(v).ok_or("sí o no")?,
            b"urgentes_en_no_molestar" => self.urgentes_pasan = si_no(v).ok_or("sí o no")?,
            b"no_molestar_desde" | b"no_molestar_hasta" => {
                let desde = clave.as_bytes()[12] == b'd' || clave.as_bytes()[12] == b'D';
                if v.is_empty() {
                    self.horario = None;
                    return Ok(());
                }
                let m = hora_minutos(v).ok_or("hora: hh:mm")?;
                let mut h = self.horario.unwrap_or(Horario { desde: m, hasta: m, dias: TODOS_LOS_DIAS });
                if desde {
                    h.desde = m;
                } else {
                    h.hasta = m;
                }
                self.horario = Some(h);
            }
            b"no_molestar_dias" => {
                let mut dias = 0u8;
                if v.eq_ignore_ascii_case("todos") {
                    dias = TODOS_LOS_DIAS;
                } else {
                    for d in v.split(|c: char| c == ' ' || c == ',').filter(|d| !d.is_empty()) {
                        // También sin tilde ("mie", "sab").
                        let k = DIAS.iter().chain(["lun", "mar", "mie", "jue", "vie", "sab", "dom"].iter()).position(|x| x.eq_ignore_ascii_case(d))
                            .ok_or("día que no existe")? % 7;
                        dias |= 1 << k;
                    }
                }
                if let Some(h) = self.horario.as_mut() {
                    h.dias = dias;
                } else if !v.is_empty() {
                    // Los días sin horario aún: se guardan para cuando lo haya (un día entero, hasta que se pongan horas).
                    self.horario = Some(Horario { desde: 0, hasta: 0, dias });
                }
            }
            b"privado" => self.privado = si_no(v).ok_or("sí o no")?,
            b"cuerpo_hace_accion" => self.cuerpo_hace_accion = si_no(v).ok_or("sí o no")?,
            b"compacto" => self.compacto = si_no(v).ok_or("sí o no")?,
            b"historial" => self.historial = si_no(v).ok_or("sí o no")?,
            b"dias_historial" => self.dias_historial = numero(v, 3650).ok_or("días: de 0 a 3650")? as u16,
            b"bloqueo" => self.bloqueo = BLOQUEOS.iter().find(|b| b.0.eq_ignore_ascii_case(v)).ok_or("nada, contador, nombres o todo")?.1,
            _ => return Err("clave que no existe"),
        }
        Ok(())
    }

    /// El valor de una clave como se escribe en el fichero, en `sal`. Devuelve cuántos bytes (0 si la clave no existe o
    /// el valor es vacío).
    pub fn valor(&self, clave: &str, sal: &mut [u8; 40]) -> usize {
        let mut n = 0;
        let mut pon = |s: &str| {
            let l = s.len().min(40 - n);
            sal[n..n + l].copy_from_slice(&s.as_bytes()[..l]);
            n += l;
        };
        let cifras = |mut x: u32, pon: &mut dyn FnMut(&str)| {
            let mut b = [0u8; 10];
            let mut i = 10;
            loop {
                i -= 1;
                b[i] = b'0' + (x % 10) as u8;
                x /= 10;
                if x == 0 {
                    break;
                }
            }
            pon(core::str::from_utf8(&b[i..]).unwrap());
        };
        let sino = |b: bool| if b { "sí" } else { "no" };
        let hora = |m: u16, pon: &mut dyn FnMut(&str)| {
            let t = [b'0' + (m / 600) as u8, b'0' + (m / 60 % 10) as u8, b':', b'0' + (m % 60 / 10) as u8, b'0' + (m % 10) as u8];
            pon(core::str::from_utf8(&t).unwrap());
        };
        match clave {
            "posicion" => pon(self.posicion.nombre()),
            "animacion" => cifras(self.animacion_ms as u32, &mut pon),
            "tiempo_baja" => cifras(self.tiempos_ms[0], &mut pon),
            "tiempo_normal" => cifras(self.tiempos_ms[1], &mut pon),
            "tiempo_urgente" => cifras(self.tiempos_ms[2], &mut pon),
            "barra_tiempo" => pon(sino(self.barra_tiempo)),
            "urgentes_en_no_molestar" => pon(sino(self.urgentes_pasan)),
            "no_molestar_desde" => {
                if let Some(h) = self.horario {
                    hora(h.desde, &mut pon);
                }
            }
            "no_molestar_hasta" => {
                if let Some(h) = self.horario {
                    hora(h.hasta, &mut pon);
                }
            }
            "no_molestar_dias" => {
                if let Some(h) = self.horario {
                    if h.dias == TODOS_LOS_DIAS {
                        pon("todos");
                    } else {
                        let mut primero = true;
                        for (k, d) in DIAS.iter().enumerate().filter(|(k, _)| h.dias & (1 << k) != 0) {
                            let _ = k;
                            if !primero {
                                pon(" ");
                            }
                            primero = false;
                            pon(d);
                        }
                    }
                }
            }
            "privado" => pon(sino(self.privado)),
            "cuerpo_hace_accion" => pon(sino(self.cuerpo_hace_accion)),
            "compacto" => pon(sino(self.compacto)),
            "historial" => pon(sino(self.historial)),
            "dias_historial" => cifras(self.dias_historial as u32, &mut pon),
            "bloqueo" => pon(BLOQUEOS.iter().find(|b| b.1 == self.bloqueo).map_or("contador", |b| b.0)),
            _ => {}
        }
        n
    }

    /// En bytes, para el servicio y la barra. Devuelve cuántos (`LARGO` como mucho).
    pub fn escribir(&self, m: &mut [u8]) -> usize {
        m[0] = VERSION;
        m[1] = self.posicion.numero();
        m[2..4].copy_from_slice(&self.animacion_ms.to_le_bytes());
        for (k, t) in self.tiempos_ms.iter().enumerate() {
            m[4 + 4 * k..8 + 4 * k].copy_from_slice(&t.to_le_bytes());
        }
        m[16] = self.barra_tiempo as u8 | (self.urgentes_pasan as u8) << 1 | (self.privado as u8) << 2 | (self.cuerpo_hace_accion as u8) << 3
            | (self.compacto as u8) << 4 | (self.historial as u8) << 5 | (self.horario.is_some() as u8) << 6;
        let h = self.horario.unwrap_or(Horario { desde: 0, hasta: 0, dias: 0 });
        m[17..19].copy_from_slice(&h.desde.to_le_bytes());
        m[19..21].copy_from_slice(&h.hasta.to_le_bytes());
        m[21] = h.dias;
        m[22..24].copy_from_slice(&self.dias_historial.to_le_bytes());
        m[24] = BLOQUEOS.iter().position(|b| b.1 == self.bloqueo).unwrap_or(1) as u8;
        25
    }

    /// De bytes, desconfiando: otra versión, otro largo o valores fuera de sitio, `None` (y quien lee se queda con lo
    /// de serie).
    pub fn leer(m: &[u8]) -> Option<Ajustes> {
        if m.len() != 25 || m[0] != VERSION {
            return None;
        }
        let u16_en = |i: usize| u16::from_le_bytes([m[i], m[i + 1]]);
        let u32_en = |i: usize| u32::from_le_bytes(m[i..i + 4].try_into().unwrap());
        let b = m[16];
        let horario = (b & 64 != 0).then(|| Horario { desde: u16_en(17), hasta: u16_en(19), dias: m[21] & TODOS_LOS_DIAS });
        if horario.is_some_and(|h| h.desde >= 1440 || h.hasta >= 1440) || u16_en(2) > 800 || (0..3).any(|k| u32_en(4 + 4 * k) > 600_000) {
            return None;
        }
        Some(Ajustes {
            posicion: POSICIONES.get(m[1] as usize)?.2,
            animacion_ms: u16_en(2),
            tiempos_ms: [u32_en(4), u32_en(8), u32_en(12)],
            barra_tiempo: b & 1 != 0,
            urgentes_pasan: b & 2 != 0,
            privado: b & 4 != 0,
            cuerpo_hace_accion: b & 8 != 0,
            compacto: b & 16 != 0,
            historial: b & 32 != 0,
            horario,
            dias_historial: u16_en(22),
            bloqueo: BLOQUEOS.get(m[24] as usize)?.1,
        })
    }

    /// Si un aviso de `urgencia` (0 baja, 1 normal, 2 urgente) se calla ahora: con "no molestar" puesto a mano
    /// (`a_mano`, del servicio) o dentro del horario (`minuto` del día y `dia`, 0 lunes, en hora local; `None` si no se
    /// sabe la hora), salvo los urgentes si pasan.
    pub fn callado(&self, a_mano: bool, ahora: Option<(u32, u32)>, urgencia: u8) -> bool {
        let horario = match (self.horario, ahora) {
            (Some(h), Some((minuto, dia))) => h.dentro(minuto, dia),
            _ => false,
        };
        (a_mano || horario) && !(urgencia >= 2 && self.urgentes_pasan)
    }

    /// Cuánto se ve un emergente de `urgencia` (0, hasta que se cierre).
    pub fn tiempo_ms(&self, urgencia: u8) -> u32 {
        self.tiempos_ms[(urgencia as usize).min(2)]
    }

    /// Las duraciones de DMS (`Theme.qml`, sobre `notificationAnimationDuration`): entrar ×0,875, salir ×0,75, mover la
    /// pila ×1, escalonar ×0,5.
    pub fn entrada_ms(&self) -> u32 {
        self.animacion_ms as u32 * 7 / 8
    }

    pub fn salida_ms(&self) -> u32 {
        self.animacion_ms as u32 * 3 / 4
    }

    pub fn pila_ms(&self) -> u32 {
        self.animacion_ms as u32
    }

    pub fn escalon_ms(&self) -> u32 {
        self.animacion_ms as u32 / 2
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn texto(a: &Ajustes, clave: &str) -> String {
        let mut b = [0u8; 40];
        let n = a.valor(clave, &mut b);
        String::from_utf8(b[..n].to_vec()).unwrap()
    }

    #[test]
    fn de_serie_como_shiva() {
        let a = Ajustes::DE_SERIE;
        assert_eq!((a.posicion, a.animacion_ms, a.tiempo_ms(0), a.tiempo_ms(1), a.tiempo_ms(2)), (Posicion::ArribaCentro, 400, 5000, 5000, 0));
        assert_eq!((a.entrada_ms(), a.salida_ms(), a.pila_ms(), a.escalon_ms()), (350, 300, 400, 200));
    }

    #[test]
    fn ida_y_vuelta() {
        let mut a = Ajustes::DE_SERIE;
        for (c, v) in [("posicion", "abajo-izquierda"), ("animacion", "0"), ("tiempo_normal", "8000"), ("barra_tiempo", "sí"), ("no_molestar_desde", "22:30"),
            ("no_molestar_hasta", "07:00"), ("no_molestar_dias", "lun mar mié jue vie"), ("dias_historial", "0"), ("bloqueo", "nombres"), ("privado", "si")]
        {
            a.poner(c, v).unwrap();
        }
        let mut m = [0u8; LARGO];
        let n = a.escribir(&mut m);
        assert_eq!(Ajustes::leer(&m[..n]), Some(a));
        // Y en texto: cada clave se escribe como se lee.
        let mut b = Ajustes::DE_SERIE;
        for c in CLAVES {
            b.poner(c, &texto(&a, c)).unwrap();
        }
        assert_eq!(b, a);
        assert_eq!(texto(&a, "no_molestar_dias"), "lun mar mié jue vie");
        assert_eq!(texto(&a, "no_molestar_desde"), "22:30");
        // La posición con el número de DMS.
        a.poner("posicion", "-1").unwrap();
        assert_eq!(a.posicion, Posicion::ArribaCentro);
        a.poner("Posicion", "5").unwrap();
        assert_eq!(a.posicion, Posicion::AbajoCentro);
    }

    #[test]
    fn valores_malos() {
        let mut a = Ajustes::DE_SERIE;
        let antes = a;
        for (c, v) in [("posicion", "en medio"), ("posicion", "4"), ("animacion", "801"), ("animacion", "-5"), ("tiempo_baja", "999999999999"), ("barra_tiempo", "quizá"),
            ("no_molestar_desde", "25:00"), ("no_molestar_desde", "7"), ("no_molestar_hasta", "07:60"), ("no_molestar_dias", "lunes"), ("bloqueo", "medio"), ("nada", "1"),
            ("dias_historial", "99999")]
        {
            assert!(a.poner(c, v).is_err(), "{} = {}", c, v);
        }
        assert_eq!(a, antes);
        // Bytes que no cuadran: lo de serie.
        let mut m = [0u8; LARGO];
        let n = a.escribir(&mut m);
        assert!(Ajustes::leer(&m[..n - 1]).is_none());
        let mut x = m;
        x[0] = 9;
        assert!(Ajustes::leer(&x[..n]).is_none());
        let mut x = m;
        x[1] = 40;
        assert!(Ajustes::leer(&x[..n]).is_none());
        let mut x = m;
        x[2..4].copy_from_slice(&5000u16.to_le_bytes());
        assert!(Ajustes::leer(&x[..n]).is_none());
        let mut x = m;
        x[16] |= 64;
        x[17..19].copy_from_slice(&3000u16.to_le_bytes());
        assert!(Ajustes::leer(&x[..n]).is_none());
        // Basura de todos los tamaños: nunca rompe.
        let mut s = 12345u32;
        for largo in 0..40 {
            let mut b = [0u8; 40];
            for x in b.iter_mut() {
                s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
                *x = (s >> 16) as u8;
            }
            let _ = Ajustes::leer(&b[..largo]);
        }
    }

    #[test]
    fn horario_y_medianoche() {
        let noche = Horario { desde: 22 * 60, hasta: 7 * 60, dias: 0b0001_1111 };
        // Lunes a las 23:00: sí. Martes a las 06:59 (empezó el lunes): sí; a las 07:00, no.
        assert!(noche.dentro(23 * 60, 0));
        assert!(noche.dentro(6 * 60 + 59, 1));
        assert!(!noche.dentro(7 * 60, 1));
        // Sábado a las 23:00: no (empieza solo de lunes a viernes); sábado a las 03:00, sí (empezó el viernes).
        assert!(!noche.dentro(23 * 60, 5));
        assert!(noche.dentro(3 * 60, 5));
        // Lunes a las 03:00: la noche del domingo no está: no.
        assert!(!noche.dentro(3 * 60, 0));
        let dia = Horario { desde: 9 * 60, hasta: 17 * 60, dias: TODOS_LOS_DIAS };
        assert!(dia.dentro(9 * 60, 3) && !dia.dentro(17 * 60, 3) && !dia.dentro(8 * 60, 3));
        let entero = Horario { desde: 0, hasta: 0, dias: 1 };
        assert!(entero.dentro(0, 0) && entero.dentro(1439, 0) && !entero.dentro(0, 1));
        // Días y minutos raros no rompen.
        assert!(!noche.dentro(u32::MAX, u32::MAX) || noche.dentro(u32::MAX, u32::MAX));
    }

    #[test]
    fn callado() {
        let mut a = Ajustes::DE_SERIE;
        assert!(!a.callado(false, Some((600, 0)), 1));
        assert!(a.callado(true, None, 1));
        // Los urgentes pasan, salvo que se diga que no.
        assert!(!a.callado(true, None, 2));
        a.urgentes_pasan = false;
        assert!(a.callado(true, None, 2));
        a.poner("no_molestar_desde", "22:00").unwrap();
        a.poner("no_molestar_hasta", "07:00").unwrap();
        assert!(a.callado(false, Some((23 * 60, 2)), 0));
        assert!(!a.callado(false, Some((12 * 60, 2)), 0));
        // Sin hora no se sabe si es de noche: no se calla por el horario.
        assert!(!a.callado(false, None, 0));
    }
}
