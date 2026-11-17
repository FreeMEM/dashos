//! S37: los iconos de un tema de Linux (norma de freedesktop) que el usuario instaló en `Home:Iconos/` y eligió en
//! Ajustes > Apariencia (`Escritorio/iconos` de `ENV:`, o el guardado de `ENVARC:`). Sin elección (lo de serie: DashOS no trae ningún tema), nada de esto
//! hace nada y salen los de la colección de DashOS.
//!
//! Se carga como las miniaturas: un icono por vuelta del bucle, de lo que se ve, y se guarda ya pintado a su tamaño
//! (`iconos_linux`). Lo que no se encuentra o no se puede pintar se recuerda como `None`, y sale el de DashOS. El
//! fichero lo decodifica el explorador (S6b, regla 5), con los topes de `kit::iconos::freedesktop` y `svg`.

use alloc::string::String;
use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::archivos as a;
use kit::iconos::freedesktop as fd;

use crate::iconos::{nombre_de, Fichero};
use crate::logica::Lugar;
use crate::ops::{self, Fs};
use crate::pintar::ICONO;
use crate::{App, RAIZ_HOME};

/// Cuántos iconos de tema se guardan pintados, como mucho.
const MAX_GUARDADOS: usize = 96;

fn base() -> Lugar {
    Lugar::raiz(RAIZ_HOME).hijo(fd::CAJON_TEMAS)
}

/// Si existe el fichero `ruta` (desde `Home:Iconos/`, de `fd::buscar`).
fn existe(fs: &mut impl Fs, ruta: &str) -> bool {
    let Some((cajones, nombre)) = ruta.rsplit_once('/') else { return false };
    let mut l = base();
    for c in cajones.split('/') {
        l = l.hijo(c);
    }
    let Ok(dir) = ops::abrir(fs, &l) else { return false };
    let r = fs.pedir(a::BUSCAR, dir, 0, 0, nombre.as_bytes());
    if r.e == a::BIEN {
        ops::cerrar(fs, r.asa);
    }
    ops::cerrar(fs, dir);
    r.e == a::BIEN && r.tipo() == a::FICHERO
}

fn leer(fs: &mut impl Fs, ruta: &str) -> Option<Vec<u8>> {
    let (cajones, nombre) = ruta.rsplit_once('/')?;
    let mut l = base();
    for c in cajones.split('/') {
        l = l.hijo(c);
    }
    let dir = ops::abrir(fs, &l).ok()?;
    let r = ops::leer_entero(fs, dir, nombre, fd::MAX_ICONO as u64);
    ops::cerrar(fs, dir);
    r.ok()
}

impl App {
    /// El texto de la elección: de `ENV:` (en uso) o, si no está, de `ENVARC:` (guardado).
    fn leer_eleccion(&mut self) -> Option<Vec<u8>> {
        let (cajon, fichero) = fd::FICHERO_ELECCION.rsplit_once('/')?;
        for asignacion in ["ENV", "ENVARC"] {
            let Some(Ok(l)) = self.lugar_por_nombre(&alloc::format!("{}:{}", asignacion, cajon)) else { continue };
            let mut fs = self.fs();
            let Ok(d) = ops::abrir(&mut fs, &l) else { continue };
            let r = ops::leer_pequeno(&mut fs, d, fichero);
            ops::cerrar(&mut fs, d);
            if let Ok(b) = r {
                return Some(b);
            }
        }
        None
    }

    /// Lee la elección y, si es un tema, su cadena (él, los que hereda y `hicolor`).
    pub fn cargar_tema_iconos(&mut self) {
        let eleccion = self.leer_eleccion().and_then(|b| fd::eleccion_de(&b));
        let cadena = {
            let mut fs = self.fs();
            match &eleccion {
                Some(t) => fd::cadena(t, &mut |n| {
                    let d = ops::abrir(&mut fs, &base().hijo(n)).ok()?;
                    let r = ops::leer_entero(&mut fs, d, "index.theme", fd::MAX_INDEX as u64);
                    ops::cerrar(&mut fs, d);
                    fd::leer_index(n, &r.ok()?).ok()
                }),
                None => Vec::new(),
            }
        };
        match &eleccion {
            None => println!("  [explorador] iconos: los de DashOS"),
            Some(t) if cadena.first().is_some_and(|c| &c.cajon == t) => {
                let nombres: Vec<&str> = cadena.iter().map(|c| c.cajon.as_str()).collect();
                println!("  [explorador] iconos: tema de Linux {} ({})", t, nombres.join(", "));
            }
            Some(t) => println!("  [explorador] iconos: el tema {} no está en Home:Iconos o no se lee; los de DashOS", t),
        }
        self.tema_linux = if eleccion.is_some() && cadena.first().is_some_and(|c| Some(&c.cajon) == eleccion.as_ref()) { cadena } else { Vec::new() };
        self.iconos_linux.clear();
    }

    /// El siguiente icono del tema que falte de lo que se ve (uno por vuelta). Verdad si hizo uno.
    pub fn un_icono_linux(&mut self) -> bool {
        if self.tema_linux.is_empty() {
            return false;
        }
        let mut falta = None;
        'fuera: for v in &self.ventanas {
            let p = v.p();
            let lado = if p.lista { 28 } else { ICONO };
            for &i in &p.vista {
                let c = &p.cosas[i];
                let n = nombre_de(&Fichero::de(&c.e, &c.donde));
                if !self.iconos_linux.contains_key(&(n, lado)) {
                    falta = Some((n, lado));
                    break 'fuera;
                }
            }
        }
        let Some((n, lado)) = falta else { return false };
        let nombres: Vec<&'static str> = self.iconos.buscar(n).map(|i| self.iconos.nombres_linux(i).to_vec()).unwrap_or_default();
        let temas = core::mem::take(&mut self.tema_linux);
        let texto = self.tema.sobre_superficie;
        let mut hecho: Option<(String, kit::imagen::Imagen)> = None;
        {
            let mut fs = self.fs();
            for nl in &nombres {
                let Some(ruta) = fd::buscar(&temas, nl, lado as u32, &mut |r| existe(&mut fs, r)) else { continue };
                match leer(&mut fs, &ruta).map(|b| fd::pintar(&ruta, &b, lado, texto)) {
                    Some(Ok(img)) => {
                        hecho = Some((ruta, img));
                        break;
                    }
                    Some(Err(porque)) => println!("  [explorador] icono de tema {}: no se pinta ({})", ruta, porque),
                    None => println!("  [explorador] icono de tema {}: no se lee", ruta),
                }
            }
        }
        self.tema_linux = temas;
        println!("  [explorador] icono de tema para {} a {}: {}", n, lado, hecho.as_ref().map_or("el de DashOS", |h| h.0.as_str()));
        if self.iconos_linux.len() >= MAX_GUARDADOS {
            self.iconos_linux.clear();
        }
        self.iconos_linux.insert((n, lado), hecho.map(|h| h.1));
        self.presentar_todas();
        true
    }
}
