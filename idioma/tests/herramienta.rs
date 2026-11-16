//! Lo que hace `tools/catalogo.py` (en Python) lo lee el crate (en Rust): los dos escriben y leen los mismos
//! formatos. Lo de `build/Locale` lo deja `make -C kernel catalogos`; si no está, esas pruebas no miran nada.

use idioma::fecha::{cadenas, CASTELLANO};
use idioma::{Catalogo, Idiomas, Lector, Pais, Pedido, Prefs};
use std::path::PathBuf;

fn raiz() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn hecho(camino: &str) -> Option<Vec<u8>> {
    let f = raiz().join("build/Locale").join(camino);
    let datos = std::fs::read(&f).ok();
    if datos.is_none() {
        eprintln!("sin {}: make -C kernel catalogos", f.display());
    }
    datos
}

#[test]
fn el_cd_del_idioma_es_el_castellano_del_codigo() {
    let cd = std::fs::read_to_string(raiz().join("locale/idiomas/idioma.cd")).unwrap();
    let lineas: Vec<&str> = cd.lines().filter(|l| !l.starts_with(';')).collect();
    assert_eq!(lineas.len(), 100);
    for (i, par) in lineas.chunks(2).enumerate() {
        assert_eq!(par[0], format!("MSG_{0} ({0}//)", i + 1));
        assert_eq!(par[1], CASTELLANO[i], "MSG_{}", i + 1);
    }
}

#[test]
fn los_paises_de_la_herramienta() {
    let Some(es) = hecho("Countries/españa.country") else { return };
    // El de España es, byte a byte, el de serie del código.
    assert_eq!(es, Pais::espana().escribir());
    let gb = Pais::leer(&hecho("Countries/great_britain.country").unwrap()).unwrap();
    assert_eq!((gb.moneda(123_456).as_str(), gb.moneda(-50).as_str(), gb.numero(1_234_567).as_str()), ("£1,234.56", "-£0.50", "1,234,567"));
    let us = Pais::leer(&hecho("Countries/united_states.country").unwrap()).unwrap();
    assert_eq!((us.moneda(123_456).as_str(), us.calendario, us.fecha_corta.as_str()), ("$1,234.56", 0, "%m/%d/%Y"));
}

#[test]
fn el_ingles_de_la_herramienta() {
    let Some(idioma) = hecho("Languages/english.language") else { return };
    let c = Catalogo::leer(&idioma, Pedido { programa: "english.language", version: 0 }).unwrap();
    assert_eq!((c.cuantos(), c.texto(cadenas::DIA_1), c.texto(cadenas::MES_1 + 10), c.texto(cadenas::COMILLA_ABRE)), (50, Some("Sunday"), Some("November"), Some("“")));
    let kit = Catalogo::leer(&hecho("Catalogs/english/kit.catalog").unwrap(), Pedido { programa: "kit", version: 1 }).unwrap();
    assert_eq!((kit.idioma.as_str(), kit.texto(1), kit.malos, kit.repetidos), ("english", Some("yes"), 0, 0));
}

/// Un lector sobre `build/Locale` como `LOCALE:` y un `locale.prefs` hecho con el crate.
struct EnDisco(Vec<u8>);

impl Lector for EnDisco {
    fn leer(&mut self, camino: &str, tope: usize) -> Result<(Vec<u8>, idioma::Firma), idioma::Falta> {
        let datos = match camino.split_once(':') {
            Some(("ENV", "Sys/locale.prefs")) => Some(self.0.clone()),
            Some(("LOCALE", resto)) => std::fs::read(raiz().join("build/Locale").join(resto)).ok(),
            _ => None,
        };
        match datos {
            Some(d) if d.len() <= tope => Ok((d.clone(), (1, d.len() as u64, 0))),
            Some(_) => Err(idioma::Falta::Otra(-11)),
            None => Err(idioma::Falta::NoExiste),
        }
    }

    fn firma(&mut self, _: &str) -> Option<idioma::Firma> {
        None
    }
}

#[test]
fn una_sesion_inglesa_con_lo_de_la_herramienta() {
    if hecho("Countries/great_britain.country").is_none() {
        return;
    }
    // Sin los datos del país en el locale.prefs: salen de LOCALE:Countries/great_britain.country.
    let mut l = EnDisco(Prefs { pais: "great_britain".into(), idiomas: vec!["english".into()], gmt: 0, flags: 0, datos: None }.escribir());
    let i = Idiomas::cargar(&mut l);
    // 2026-11-15 17:05:09 UTC.
    assert_eq!(i.local.fecha_larga(1_794_762_309), "Sunday 15 November 2026");
    assert_eq!(i.local.moneda(-1_050), "-£10.50");
    assert_eq!(i.catalogo(&mut l, "kit", 1, "español").unwrap().texto(2), Some("no"));
    // De otra versión del kit: no vale.
    assert!(i.catalogo(&mut l, "kit", 2, "español").is_none());
}
