//! S27, oleada 3: el "Dash" del reloj de la barra, como el de DMS: un panel bajo la barra, en el centro, con su barra
//! de pestañas (aquí, solo Resumen) y una rejilla de tarjetas: el reloj, quien está delante, los avisos actuales, el
//! calendario del mes (con la zona horaria del sistema) y el sistema (CPU y memoria). El tiempo y la música, que DMS
//! pone en el Resumen, no tienen dato en DashOS: van, en su sitio, quien está delante y el sistema (las tarjetas de DMS
//! que se pueden añadir).
//!
//! Portado de DMS (DankMaterialShell v2.1.2: `Modules/DDash/DDashPopout.qml`, `DashMetrics.qml`, `OverviewTab.qml`,
//! `Overview/*Card.qml` y `DCommon/Widgets/DMonthGrid.qml`; MIT, copyright (c) 2025 Avenge Media LLC; aviso entero en
//! `docs/licencias/DankMaterialShell-MIT.txt`): 6 columnas de 128 y filas de 96 con 8 entre ellas, 12 de relleno, las
//! pestañas de 64, el calendario de 6 semanas que empieza el lunes, hoy en el primario (y con un anillo de 2 si se elige
//! otro día), los días de otros meses atenuados, y abrir con la curva expresiva subiendo 16. La capa ocupa todo el alto
//! (`fullHeightSurface`): un clic fuera de la tarjeta lo cierra.

extern crate alloc;

use crate::avisos_vista::{Capa, Letras, Reloj, Sitio};
use crate::calendario::{rejilla, Dia, CORTOS, MESES};
use crate::dms_medidas as dm;
use crate::ventana_cliente::{Cliente, VA};
use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::avisos as av;
use kit::lienzo::{Color, Lienzo, Rect};
use kit::tema::{apagado, con_opacidad, Tema};
use kit::texto::Fuente;

const SUP: u32 = 110;
const CAPA: u32 = 111;
const POOL: u32 = 112;
const BUFER: u32 = 113;
const BUFER_2: u32 = 114;
const VA_DASH: usize = VA + (10 << 32);
/// `DashMetrics`: columna de 128, fila de 96, 8 entre ellas, 12 de relleno, pestañas de 64.
const COLUMNA: i32 = 128;
const FILA: i32 = 96;
const HUECO: i32 = 8;
const RELLENO: i32 = dm::M;
const PESTANAS: i32 = 64;
/// El ancho de DMS (832), o el de la pantalla menos 32 si no cabe (`DashMetrics.widthFor`).
const ANCHO_DMS: i32 = 6 * COLUMNA + 5 * HUECO + 2 * RELLENO;
const ALTO_MAX: i32 = 1600;
/// A cuánto de la barra (`popupDistance`).
const DISTANCIA: i32 = dm::XS;

/// Las tarjetas de la rejilla: columna, fila, ancho y alto (en celdas).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tarjeta {
    Reloj,
    Usuario,
    Avisos,
    Calendario,
    Sistema,
}

const TARJETAS: [(Tarjeta, i32, i32, i32, i32); 5] = [
    (Tarjeta::Reloj, 0, 0, 2, 1),
    (Tarjeta::Usuario, 2, 0, 1, 1),
    (Tarjeta::Avisos, 3, 0, 3, 5),
    (Tarjeta::Calendario, 0, 1, 3, 3),
    (Tarjeta::Sistema, 0, 4, 3, 1),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zona {
    Antes,
    Despues,
    Hoy,
    Dia(usize),
    Avisos,
}

/// El Dash abierto.
pub struct Dash {
    capa: Capa,
    ancho: i32,
    /// El alto que da el compositor a la capa (todo el de la pantalla bajo la barra) y el de la tarjeta.
    alto: i32,
    /// El día elegido (al abrir, hoy) y hoy.
    elegido: Dia,
    hoy: Option<Dia>,
    abre_us: u64,
    avisos: Vec<av::Aviso>,
    persona: String,
    uso: (Option<u32>, Option<u32>),
    zonas: Vec<(Rect, Zona)>,
    tarjeta: Rect,
}

/// Qué hacer tras un clic o una tecla.
pub enum Hecho {
    Nada,
    Cerrar,
    /// Cerrar y abrir el centro de avisos.
    CentroDeAvisos,
}

fn hoy(reloj: &Reloj) -> Option<Dia> {
    (reloj.ns > 0).then(|| {
        let f = crate::reloj::fecha(dashos::zona::local_ns(reloj.zona, reloj.ns));
        Dia::new(f.anio, f.mes, f.dia)
    })
}

impl Dash {
    /// Lo abre bajo la barra, en el centro, con el teclado.
    pub fn abrir(c: &mut Cliente, reloj: &Reloj, ancho_pantalla: i32, avisos: Vec<av::Aviso>, persona: String, uso: (Option<u32>, Option<u32>)) -> Option<Dash> {
        let ancho = ANCHO_DMS.min(ancho_pantalla - 2 * dm::L).max(400);
        // Arriba y abajo (todo el alto), sin izquierda ni derecha: en el centro.
        let sitio = Sitio { nombre: "dash", anclas: 1 | 2, margenes: [0, 0, 0, 0], teclado: 1 };
        let capa = Capa::abrir(c, (SUP, CAPA, POOL, BUFER, BUFER_2), VA_DASH, ancho, 0, ALTO_MAX, &sitio)?;
        let hoy = hoy(reloj);
        println!("  [barra] Dash abierto: hoy {}; {} avisos actuales", hoy.map_or(String::from("sin hora"), |d| alloc::format!("{}-{:02}-{:02}", d.anio, d.mes, d.dia)), avisos.len());
        Some(Dash {
            capa,
            ancho,
            alto: 0,
            elegido: hoy.unwrap_or(Dia::new(2026, 1, 1)),
            hoy,
            abre_us: dashos::ahora_us(),
            avisos,
            persona,
            uso,
            zonas: Vec::new(),
            tarjeta: Rect::new(0, 0, 0, 0),
        })
    }

    pub fn cerrar(self, c: &mut Cliente) {
        self.capa.cerrar(c);
        println!("  [barra] Dash cerrado");
    }

    pub fn suya(&self, sup: u32) -> bool {
        sup == SUP
    }

    pub fn es(&self, capa: u32) -> bool {
        self.capa.es(capa)
    }

    pub fn animando(&self) -> bool {
        dashos::ahora_us() < self.abre_us + 400_000
    }

    /// Su configure: el alto que le toca. Se pinta y se dice dónde está cada cosa (para las pruebas).
    pub fn configurar(&mut self, c: &mut Cliente, serie: u32, alto: i32, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        self.capa.configurar(c, serie, alto);
        let nuevo = alto != self.alto;
        self.alto = alto;
        self.pintar(c, t, le, reloj);
        if nuevo {
            let z = |q: Zona| self.zonas.iter().find(|x| x.1 == q).map(|x| (x.0.x + x.0.w / 2, x.0.y + x.0.h / 2)).unwrap_or((0, 0));
            let (a, d, h) = (z(Zona::Antes), z(Zona::Despues), z(Zona::Hoy));
            println!("  [barra] Dash: tarjeta de {} × {}; en su capa: mes anterior ({}, {}), siguiente ({}, {}), hoy ({}, {})", self.tarjeta.w, self.tarjeta.h, a.0, a.1, d.0, d.1, h.0, h.1);
        }
    }

    /// Dónde va cada tarjeta.
    fn cajas(&self) -> (Rect, [(Tarjeta, Rect); 5]) {
        // El alto: el de DMS si cabe; si no, las filas se encogen para caber (DMS haría desplazable la rejilla).
        let libre = (self.alto - 2 * DISTANCIA).max(200);
        let fila = FILA.min((libre - PESTANAS - RELLENO - 2 * RELLENO - 4 * HUECO) / 5).max(48);
        let alto = PESTANAS + RELLENO + 5 * fila + 4 * HUECO + 2 * RELLENO;
        let tarjeta = Rect::new(0, DISTANCIA, self.ancho, alto);
        let columna = (self.ancho - 2 * RELLENO - 5 * HUECO) / 6;
        let x0 = RELLENO;
        let y0 = DISTANCIA + RELLENO + PESTANAS + RELLENO;
        let cajas = TARJETAS.map(|(t, c, f, w, h)| {
            (t, Rect::new(x0 + c * (columna + HUECO), y0 + f * (fila + HUECO), w * columna + (w - 1) * HUECO, h * fila + (h - 1) * HUECO))
        });
        (tarjeta, cajas)
    }

    pub fn pintar(&mut self, c: &mut Cliente, t: &Tema, le: &mut Letras, reloj: &Reloj) {
        if self.alto <= 0 {
            return;
        }
        let (tarjeta, cajas) = self.cajas();
        self.tarjeta = tarjeta;
        let mut zonas = Vec::new();
        let abre = self.abre_us;
        let (alto, ancho) = (self.alto, self.ancho);
        let mut capa = core::mem::replace(&mut self.capa, Capa::vacia());
        let yo: &Dash = self;
        capa.pintar(c, alto, |px, alto| {
            // Al abrir: sube 16 hasta su sitio y aparece, en 250 ms con la curva expresiva (`DPopoutHost`).
            let p = dm::EXPRESIVA_ESPACIAL.en(dm::avance(abre, dashos::ahora_us(), 250));
            let dy = dm::entre(16, 0, p);
            {
                let mut l = Lienzo::new(&mut *px, ancho, alto, ancho as usize);
                let tr = Rect::new(tarjeta.x, tarjeta.y + dy, tarjeta.w, tarjeta.h);
                l.redondeado(tr, dm::RADIO_VENTANA, t.superficie);
                l.borde(tr, dm::RADIO_VENTANA, 1, t.variante_contorno);
                pintar_pestanas(&mut l, Rect::new(RELLENO, tr.y + RELLENO, ancho - 2 * RELLENO, PESTANAS), t, le);
                for (q, r) in cajas {
                    let r = Rect::new(r.x, r.y + dy, r.w, r.h);
                    yo.pintar_tarjeta(&mut l, q, r, t, le, reloj, &mut zonas);
                }
            }
            crate::avisos_vista::desvanecer_pub(px, ancho, alto, Rect::new(0, 0, ancho, alto), p.min(1.0));
        });
        self.capa = capa;
        self.zonas = zonas;
    }

    #[allow(clippy::too_many_arguments)]
    fn pintar_tarjeta(&self, l: &mut Lienzo, q: Tarjeta, r: Rect, t: &Tema, le: &mut Letras, reloj: &Reloj, zonas: &mut Vec<(Rect, Zona)>) {
        let radio = dm::radio(12);
        match q {
            Tarjeta::Reloj => {
                // `ClockCard` con el tono primario: el contenedor primario y las cifras, lo más grandes que quepan.
                l.redondeado(r, radio, t.contenedor_primario);
                let hora = match reloj.ns {
                    0 => String::from("--:--"),
                    ns => {
                        let f = crate::reloj::fecha(dashos::zona::local_ns(reloj.zona, ns));
                        alloc::format!("{:02}:{:02}", f.hora, f.minuto)
                    }
                };
                let mut tam = ((r.h - 2 * dm::L) as u32 * 10 / 8).max(16);
                while tam > 16 && le.m.medir(&hora, tam).ancho > r.w - 2 * dm::L {
                    tam -= 2;
                }
                let m = le.m.medir(&hora, tam);
                le.m.dibujar(l, r.x + (r.w - m.ancho) / 2, r.y + (r.h + m.sube - m.baja) / 2, &hora, tam, t.sobre_contenedor_primario);
            }
            Tarjeta::Usuario => {
                // Estrecha (menos de 192): solo el avatar, centrado.
                l.redondeado(r, radio, t.contenedor());
                let lado = 56.min(r.h - 16);
                let a = Rect::new(r.x + (r.w - lado) / 2, r.y + (r.h - lado) / 2, lado, lado);
                l.redondeado(a, lado / 2, t.contenedor_primario);
                let nombre = if self.persona.is_empty() { "DashOS" } else { self.persona.as_str() };
                let inicial: String = nombre.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                let m = le.m.medir(&inicial, 20);
                le.m.dibujar(l, a.x + (a.w - m.ancho) / 2, a.y + a.h / 2 + 7, &inicial, 20, t.sobre_contenedor_primario);
            }
            Tarjeta::Sistema => {
                // `SystemMonitorCard` ancha: una fila por medida con su barra de 12 (al 10 % del texto) y su valor.
                l.redondeado(r, radio, t.contenedor());
                let filas = [("CPU", self.uso.0, 600, 800), ("Memoria", self.uso.1, 750, 900)];
                let alto_fila = (r.h - 2 * dm::S) / 2;
                for (k, (nombre, valor, aviso, error)) in filas.into_iter().enumerate() {
                    let y = r.y + dm::S + k as i32 * alto_fila;
                    le.m.dibujar(l, r.x + dm::M, y + alto_fila / 2 + 5, nombre, dm::LETRA_MEDIANA, t.sobre_superficie);
                    let texto = valor.map_or(String::from("–"), |d| alloc::format!("{} %", (d + 5) / 10));
                    let ancho_valor = le.m.medir("100 %", dm::LETRA_MEDIANA).ancho;
                    let barra = Rect::new(r.x + 90, y + (alto_fila - 12) / 2, r.w - 90 - dm::M - ancho_valor - dm::M, 12);
                    l.redondeado(barra, 6, con_opacidad(t.sobre_superficie, 26));
                    if let Some(d) = valor {
                        let color = if d > error { t.error } else if d > aviso { t.contenedor_terciario } else { t.primario };
                        let w = (barra.w * d.min(1000) as i32 / 1000).max(12);
                        l.redondeado(Rect::new(barra.x, barra.y, w, 12), 6, color);
                    }
                    let wv = le.m.medir(&texto, dm::LETRA_MEDIANA).ancho;
                    le.m.dibujar(l, r.x + r.w - dm::M - wv, y + alto_fila / 2 + 5, &texto, dm::LETRA_MEDIANA, t.sobre_superficie);
                }
            }
            Tarjeta::Avisos => {
                // Los actuales (la de DMS enseña su centro): una fila por aviso; sin ninguno, "Estás al día".
                l.redondeado(r, radio, t.contenedor());
                zonas.push((r, Zona::Avisos));
                let titulo = alloc::format!("Avisos · {}", self.avisos.len());
                le.m.dibujar(l, r.x + dm::M, r.y + dm::M + 14, &titulo, dm::LETRA_MEDIANA, t.sobre_superficie);
                if self.avisos.is_empty() {
                    let texto = "Estás al día";
                    let w = le.m.medir(texto, dm::LETRA_GRANDE).ancho;
                    le.m.dibujar(l, r.x + (r.w - w) / 2, r.y + r.h / 2, texto, dm::LETRA_GRANDE, t.sobre_variante_superficie);
                }
                let mut y = r.y + dm::M + 28;
                for a in &self.avisos {
                    if y + 52 > r.y + r.h - dm::S {
                        break;
                    }
                    let fila = Rect::new(r.x + dm::S, y, r.w - 2 * dm::S, 52);
                    l.redondeado(fila, dm::radio(8), t.contenedor_alto());
                    let icono = Rect::new(fila.x + 8, fila.y + 10, 32, 32);
                    l.redondeado(icono, 16, t.contenedor_secundario());
                    let inicial: String = a.de().chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                    let m = le.m.medir(&inicial, 14);
                    le.m.dibujar(l, icono.x + (32 - m.ancho) / 2, icono.y + 21, &inicial, 14, t.sobre_contenedor_secundario());
                    let x = icono.x + 32 + 10;
                    let ancho = fila.x + fila.w - 8 - x;
                    let tt = recorta(le.m, a.titulo(), dm::LETRA_MEDIANA, ancho);
                    le.m.dibujar(l, x, fila.y + 22, &tt, dm::LETRA_MEDIANA, t.sobre_superficie);
                    let de = recorta(le.f, a.de(), dm::LETRA_PEQUENA, ancho);
                    le.f.dibujar(l, x, fila.y + 40, &de, dm::LETRA_PEQUENA, t.sobre_variante_superficie);
                    y += 52 + dm::XXS;
                }
            }
            Tarjeta::Calendario => self.pintar_calendario(l, r, t, le, zonas),
        }
    }

    /// `CalendarOverviewCard` con `DMonthGrid`: cabecera de 32 (anterior, "noviembre 2026", hoy y siguiente), los días
    /// de la semana en 20 y las 42 casillas a 4 una de otra.
    fn pintar_calendario(&self, l: &mut Lienzo, r: Rect, t: &Tema, le: &mut Letras, zonas: &mut Vec<(Rect, Zona)>) {
        l.redondeado(r, dm::radio(12), t.contenedor());
        let pad = dm::S;
        let dentro = Rect::new(r.x + pad, r.y + pad, r.w - 2 * pad, r.h - 2 * pad);
        let cabeza = Rect::new(dentro.x, dentro.y, dentro.w, 32);
        let antes = Rect::new(cabeza.x, cabeza.y, 32, 32);
        let despues = Rect::new(cabeza.x + cabeza.w - 32, cabeza.y, 32, 32);
        let hoy_b = Rect::new(despues.x - 32, cabeza.y, 32, 32);
        flecha(l, antes, true, t.sobre_variante_superficie);
        flecha(l, despues, false, t.sobre_variante_superficie);
        // "today": un cuadrito con un punto; apagado si ya se ve hoy elegido.
        let en_hoy = self.hoy == Some(self.elegido);
        let c_hoy = if en_hoy { apagado(t.primario) } else { t.primario };
        l.borde(Rect::new(hoy_b.x + 8, hoy_b.y + 8, 16, 16), 3, 2, c_hoy);
        l.redondeado(Rect::new(hoy_b.x + 13, hoy_b.y + 15, 6, 6), 2, c_hoy);
        zonas.push((antes, Zona::Antes));
        zonas.push((despues, Zona::Despues));
        zonas.push((hoy_b, Zona::Hoy));
        let titulo = alloc::format!("{} {}", MESES[self.elegido.mes as usize - 1], self.elegido.anio);
        let tt = recorta(le.m, &titulo, dm::LETRA_GRANDE, cabeza.w - 32 * 4);
        let w = le.m.medir(&tt, dm::LETRA_GRANDE).ancho;
        le.m.dibujar(l, cabeza.x + (cabeza.w - w) / 2, cabeza.y + 22, &tt, dm::LETRA_GRANDE, t.sobre_superficie);
        // La rejilla: 7 columnas, 20 de cabecera y 6 filas, con 4 entre casillas.
        let zona = Rect::new(dentro.x, cabeza.y + 32 + dm::S, dentro.w, dentro.h - 32 - dm::S);
        let celda_w = (zona.w - 6 * dm::XS) / 7;
        let celda_h = (zona.h - 20 - 6 * dm::XS) / 6;
        for (k, d) in CORTOS.iter().enumerate() {
            let x = zona.x + k as i32 * (celda_w + dm::XS);
            let w = le.m.medir(d, dm::LETRA_PEQUENA).ancho;
            le.m.dibujar(l, x + (celda_w - w) / 2, zona.y + 15, d, dm::LETRA_PEQUENA, t.sobre_variante_superficie);
        }
        let radio = dm::radio(8).min(celda_h.min(celda_w) / 2);
        for (i, (d, del_mes)) in rejilla(self.elegido).iter().enumerate() {
            let celda = Rect::new(zona.x + (i as i32 % 7) * (celda_w + dm::XS), zona.y + 20 + dm::XS + (i as i32 / 7) * (celda_h + dm::XS), celda_w, celda_h);
            let elegido = *d == self.elegido;
            let es_hoy = Some(*d) == self.hoy;
            let (fondo, tinta) = if elegido {
                (t.primario, t.sobre_primario)
            } else if *del_mes {
                (t.contenedor_alto(), if es_hoy { t.primario } else { t.sobre_superficie })
            } else {
                (con_opacidad(t.contenedor_alto(), 26), apagado(t.sobre_superficie))
            };
            l.redondeado(celda, radio, fondo);
            if es_hoy && !elegido {
                l.borde(celda, radio, 2, t.primario);
            }
            let n = alloc::format!("{}", d.dia);
            let w = le.f.medir(&n, dm::LETRA_MEDIANA).ancho;
            le.f.dibujar(l, celda.x + (celda.w - w) / 2, celda.y + dm::XS + 13, &n, dm::LETRA_MEDIANA, tinta);
            zonas.push((celda, Zona::Dia(i)));
        }
    }

    /// Un clic en (x, y) de su capa.
    pub fn clic(&mut self, c: &mut Cliente, x: i32, y: i32, t: &Tema, le: &mut Letras, reloj: &Reloj) -> Hecho {
        if !self.tarjeta.contiene(x, y) {
            return Hecho::Cerrar;
        }
        let z = self.zonas.iter().rev().find(|z| z.0.contiene(x, y)).map(|z| z.1);
        let antes = self.elegido;
        match z {
            Some(Zona::Antes) => self.elegido = self.elegido.mas_meses(-1),
            Some(Zona::Despues) => self.elegido = self.elegido.mas_meses(1),
            Some(Zona::Hoy) => self.elegido = self.hoy.unwrap_or(self.elegido),
            Some(Zona::Dia(i)) => self.elegido = rejilla(self.elegido)[i].0,
            Some(Zona::Avisos) => return Hecho::CentroDeAvisos,
            None => return Hecho::Nada,
        }
        if self.elegido != antes {
            println!("  [barra] Dash: el calendario va al {}-{:02}-{:02}", self.elegido.anio, self.elegido.mes, self.elegido.dia);
            self.pintar(c, t, le, reloj);
        }
        Hecho::Nada
    }

    /// Una tecla (código de Linux, pulsada): flechas (días y semanas), Re Pág y Av Pág (meses), T (hoy), Esc.
    pub fn tecla(&mut self, c: &mut Cliente, codigo: u32, t: &Tema, le: &mut Letras, reloj: &Reloj) -> Hecho {
        let antes = self.elegido;
        self.elegido = match codigo {
            1 => return Hecho::Cerrar,
            105 => self.elegido.mas(-1),
            106 => self.elegido.mas(1),
            103 => self.elegido.mas(-7),
            108 => self.elegido.mas(7),
            104 => self.elegido.mas_meses(-1),
            109 => self.elegido.mas_meses(1),
            20 => self.hoy.unwrap_or(self.elegido),
            _ => return Hecho::Nada,
        };
        if self.elegido != antes {
            println!("  [barra] Dash: el calendario va al {}-{:02}-{:02}", self.elegido.anio, self.elegido.mes, self.elegido.dia);
            self.pintar(c, t, le, reloj);
        }
        Hecho::Nada
    }
}

fn recorta(f: &mut Fuente, texto: &str, tam: u32, ancho: i32) -> String {
    if f.medir(texto, tam).ancho <= ancho {
        return String::from(texto);
    }
    let mut s = String::from(texto);
    while !s.is_empty() && f.medir(&alloc::format!("{}…", s), tam).ancho > ancho {
        s.pop();
    }
    alloc::format!("{}…", s.trim_end())
}

fn flecha(l: &mut Lienzo, r: Rect, izquierda: bool, color: Color) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    let s = if izquierda { 1 } else { -1 };
    linea(l, (cx + 2 * s, cy - 5), (cx - 3 * s, cy), 2, color);
    linea(l, (cx - 3 * s, cy), (cx + 2 * s, cy + 5), 2, color);
}

/// La barra de pestañas (`DNavigationBar`): aquí una sola, Resumen, con su indicador de 56 × 32 en el contenedor
/// secundario, el icono de 24 y su nombre en 12 medium debajo.
fn pintar_pestanas(l: &mut Lienzo, r: Rect, t: &Tema, le: &mut Letras) {
    let cx = r.x + r.w / 2;
    let ind = Rect::new(cx - 28, r.y + 6, 56, 32);
    l.redondeado(ind, 16, t.contenedor_secundario());
    for (dx, dy) in [(-7, -7), (1, -7), (-7, 1), (1, 1)] {
        l.redondeado(Rect::new(cx + dx, ind.y + 16 + dy, 6, 6), 1, t.sobre_contenedor_secundario());
    }
    let texto = "Resumen";
    let w = le.m.medir(texto, dm::LETRA_PEQUENA).ancho;
    le.m.dibujar(l, cx - w / 2, ind.y + 32 + 4 + 12, texto, dm::LETRA_PEQUENA, t.secundario);
}

/// Un trazo de `a` a `b` en píxeles, de `grosor` píxeles (el del kit va en octavos de píxel).
fn linea(l: &mut Lienzo, a: (i32, i32), b: (i32, i32), grosor: i32, color: Color) {
    l.trazo((a.0 * 8 + 4, a.1 * 8 + 4), (b.0 * 8 + 4, b.1 * 8 + 4), grosor * 8, color);
}
