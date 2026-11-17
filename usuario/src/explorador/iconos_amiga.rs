//! Los iconos `.info` de la caja Amiga en el explorador (plan de iconos, parte 3). Cada fichero o cajón del cajón de la
//! caja que tiene su `X.info` al lado se enseña con ese icono (leído por `iconos_cajas::amiga`: clásico, NewIcons,
//! GlowIcons, ARGB o PNG de OS4), y el volumen con el de su `Disk.info`. Los `.info` que son de otra cosa se esconden,
//! como los ocultos, igual que en el Workbench (se ven con "Mostrar los ocultos", cada uno con su propio icono).
//!
//! Se leen a ratos, cuando no hay otra cosa (como las miniaturas), en este proceso (S6b) y con tope de tamaño; un
//! `.info` roto se queda con el icono de siempre. Se guardan por cajón y nombre con su tamaño y su fecha: si el `.info`
//! cambia, se vuelve a leer.

use crate::*;
use iconos_cajas::amiga;
use kit::imagen::Imagen;

/// Los `.info` de más de esto no se leen (los de verdad no llegan a 100 KB; los ARGB de 256 × 256, unos 200).
const MAX_INFO: u64 = 512 * 1024;
/// Iconos guardados como mucho (unos 16 KB cada uno, de 64 × 64). Lleno, se olvidan todos y se leen otra vez los que
/// se ven; si los que se ven no caben, no se leen más (sin bucles de leer y olvidar).
const MAX_GUARDADOS: usize = 256;
/// Lo que se dedica a leer iconos en cada vuelta del bucle, como mucho.
const RATO_US: u64 = 8_000;

/// Un icono leído (o `None` si no se pudo), con el tamaño y la fecha del `.info` de cuando se leyó.
pub struct Leido {
    tam: u64,
    fecha: u64,
    icono: Option<(Imagen, Option<Imagen>)>,
}

#[derive(Default)]
pub struct IconosAmiga {
    leidos: BTreeMap<(Lugar, String), Leido>,
    /// El `Disk.info` del volumen: `None` si aún no se miró.
    disco: Option<Option<Imagen>>,
}

/// Las cosas de cada cajón por su nombre (en la búsqueda hay de varios cajones).
type PorNombre<'a> = BTreeMap<(&'a Lugar, &'a str), &'a Entrada>;

fn por_nombre(cosas: &[Cosa]) -> PorNombre<'_> {
    cosas.iter().filter(|c| c.donde.raiz == RAIZ_AMIGA).map(|c| ((&c.donde, c.e.nombre.as_str()), &c.e)).collect()
}

/// Si `nombre` (en `donde`) es un `.info` de otra cosa del mismo cajón, o el `Disk.info` del volumen: se esconde.
fn de_otro(donde: &Lugar, nombre: &str, hay: &PorNombre) -> bool {
    if donde.raiz != RAIZ_AMIGA {
        return false;
    }
    if donde.nombres.is_empty() && nombre == "Disk.info" {
        return true;
    }
    nombre.strip_suffix(".info").is_some_and(|base| !base.is_empty() && hay.contains_key(&(donde, base)))
}

/// Las cosas de `cosas` que se esconden por ser el `.info` de otra (para la vista sin ocultos).
pub fn escondidas(cosas: &[Cosa]) -> Vec<bool> {
    let hay = por_nombre(cosas);
    cosas.iter().map(|c| de_otro(&c.donde, &c.e.nombre, &hay)).collect()
}

/// El nombre del `.info` que da su icono a `nombre`: él mismo si lo es, o `nombre.info` si está en el cajón.
fn info_de(donde: &Lugar, nombre: &str, hay: &PorNombre) -> Option<String> {
    if nombre.len() > 5 && nombre.ends_with(".info") {
        return Some(String::from(nombre));
    }
    let i = alloc::format!("{nombre}.info");
    hay.contains_key(&(donde, i.as_str())).then_some(i)
}

fn leer(datos: &[u8]) -> Result<amiga::Icono, amiga::Error> {
    amiga::leer(datos)
}

impl IconosAmiga {
    /// La imagen de `c` (la de seleccionado si está elegida y la hay), si tiene `.info` y ya se leyó bien.
    pub fn de(&self, c: &Cosa, hermanos: &[Cosa], elegida: bool) -> Option<&Imagen> {
        if c.donde.raiz != RAIZ_AMIGA {
            return None;
        }
        let info = if c.e.nombre.len() > 5 && c.e.nombre.ends_with(".info") {
            String::from(c.e.nombre.as_str())
        } else {
            let i = alloc::format!("{}.info", c.e.nombre);
            hermanos.iter().any(|h| h.donde == c.donde && h.e.nombre == i).then_some(i)?
        };
        let (normal, sel) = self.leidos.get(&(c.donde.clone(), info))?.icono.as_ref()?;
        Some(if elegida { sel.as_ref().unwrap_or(normal) } else { normal })
    }

    /// El icono del volumen (su `Disk.info`), si lo hay.
    pub fn disco(&self) -> Option<&Imagen> {
        self.disco.as_ref()?.as_ref()
    }
}

impl App {
    /// Lee los `.info` que falten de lo que se ve, un rato. Verdad si leyó alguno.
    pub fn leer_iconos_amiga(&mut self) -> bool {
        let t0 = dashos::ahora_us();
        let mut hizo = false;
        while dashos::ahora_us() - t0 < RATO_US {
            let Some((donde, info, tam, fecha)) = self.info_que_falta() else { break };
            let datos = {
                let mut fs = self.fs();
                ops::abrir(&mut fs, &donde).and_then(|dir| {
                    let r = ops::leer_entero(&mut fs, dir, &info, MAX_INFO);
                    ops::cerrar(&mut fs, dir);
                    r
                })
            };
            let icono = match datos.as_deref().map_err(|_| amiga::Error::Corto).and_then(leer) {
                Ok(ic) => {
                    println!(
                        "  [explorador] icono de {}: {}x{} ({}{})",
                        texto_lugar(&donde.hijo(&info)),
                        ic.normal.ancho,
                        ic.normal.alto,
                        ic.formato.nombre(),
                        if ic.seleccionado.is_some() { ", con seleccionado" } else { "" }
                    );
                    Some((ic.normal, ic.seleccionado))
                }
                Err(e) => {
                    println!("  [explorador] icono de {}: no se lee ({:?}), el de siempre", texto_lugar(&donde.hijo(&info)), e);
                    None
                }
            };
            self.iconos_amiga.leidos.insert((donde, info), Leido { tam, fecha, icono });
            hizo = true;
        }
        // El del volumen, con la primera ventana (al arrancar, el servicio de archivos tiene bastante con los demás).
        if self.iconos_amiga.disco.is_none() && !self.ventanas.is_empty() {
            let datos = {
                let mut fs = self.fs();
                ops::abrir(&mut fs, &Lugar::raiz(RAIZ_AMIGA)).and_then(|dir| {
                    let r = ops::leer_entero(&mut fs, dir, "Disk.info", MAX_INFO);
                    ops::cerrar(&mut fs, dir);
                    r
                })
            };
            let img = datos.ok().and_then(|d| leer(&d).ok()).map(|ic| ic.normal);
            if let Some(i) = &img {
                println!("  [explorador] icono del volumen Amiga: {}x{}", i.ancho, i.alto);
            }
            self.iconos_amiga.disco = Some(img);
            hizo = true;
        }
        if hizo {
            self.presentar_todas();
        }
        hizo
    }

    /// El siguiente `.info` por leer de lo que se ve: (cajón, nombre, tamaño, fecha).
    fn info_que_falta(&mut self) -> Option<(Lugar, String, u64, u64)> {
        let mut falta = None;
        let mut hacen_falta = 0;
        for v in &self.ventanas {
            let p = v.p();
            if !p.cosas.iter().any(|c| c.donde.raiz == RAIZ_AMIGA) {
                continue;
            }
            let hay = por_nombre(&p.cosas);
            for &i in &p.vista {
                let c = &p.cosas[i];
                let Some(info) = info_de(&c.donde, &c.e.nombre, &hay) else { continue };
                let Some((cajon, tam, fecha)) = hay.get(&(&c.donde, info.as_str())).map(|e| (e.cajon, e.tam, e.fecha)) else { continue };
                if cajon {
                    continue;
                }
                hacen_falta += 1;
                let al_dia = self.iconos_amiga.leidos.get(&(c.donde.clone(), info.clone())).is_some_and(|l| l.tam == tam && l.fecha == fecha);
                if !al_dia && falta.is_none() {
                    falta = Some((c.donde.clone(), info, tam, fecha));
                }
            }
        }
        let falta = falta?;
        if self.iconos_amiga.leidos.len() >= MAX_GUARDADOS {
            // Si lo que se ve cabe, se empieza de nuevo con ello; si no, se deja como está.
            if hacen_falta >= MAX_GUARDADOS {
                return None;
            }
            self.iconos_amiga.leidos.clear();
        }
        Some(falta)
    }
}
