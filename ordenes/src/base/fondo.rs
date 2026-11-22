//! `Fondo`: el fondo de escritorio desde el Shell (rama `fondos`). Escribe `ENV:Escritorio/fondo` (y, con `GUARDAR`,
//! `ENVARC:`), el mismo fichero que la herramienta de fondos y que se puede editar a mano: el programa `fondo` lo vigila
//! y lo pone en marcha, con su transición.
//!
//! - `Fondo` o `Fondo ABRIR`: abre la herramienta de fondos (la pide por `ENV:Escritorio/fondo-pide`).
//! - `Fondo LISTA`: los fondos que hay (`Home:Fondos`, `SYS:Fondos` y los animados) y los ajustes de ahora.
//! - `Fondo <imagen>`: ese fondo (`atardecer.png` es de `Home:Fondos`; `SYS:Fondos/mar.png`; `animado:aurora`). Con
//!   `OSCURO`, solo el del modo oscuro (con `CLARO`, solo el del claro; sin ninguno, el de los dos); con
//!   `PANTALLA DASHOS-2`, el de esa pantalla.
//! - `Fondo SIGUIENTE`: el siguiente de la carpeta del pase.
//! - `Fondo AJUSTE mosaico`, `Fondo TRANSICION disolver`, `Fondo PASE 5`, `Fondo EFECTO nieve`, `Fondo CPU 2`.

use crate::{di, Orden};
use alloc::string::String;
use alloc::vec::Vec;
use kit::fondos::ajustes::{self, Config};
use kit::fondos::{animado, Ajuste};

const PIDE: &str = "Escritorio/fondo-pide";

fn leer_config(o: &mut Orden) -> (Config, Option<Vec<u8>>) {
    for asignacion in ["ENV", "ENVARC"] {
        if let Ok(Some(b)) = super::ajuste::leer(o, &alloc::format!("{}:{}", asignacion, ajustes::FICHERO)) {
            let (c, avisos) = ajustes::de_texto(&b);
            for a in avisos {
                di!(o, "Aviso: {}: {}\n", ajustes::FICHERO, a);
            }
            return (c, Some(b));
        }
    }
    // Sin fichero aún: lo de antes (`Home:Prefs/fondo`, el nombre de un PNG de `Home:Fondos`), que es lo que se ve.
    let mut c = Config::default();
    if let Ok(Some(b)) = super::ajuste::leer(o, "Home:Prefs/fondo")
        && let Ok(t) = core::str::from_utf8(&b)
        && let Some(n) = t.lines().next().map(str::trim).filter(|n| ajustes::nombre_vale(n) && !n.contains(':'))
    {
        c.fondo = ajustes::camino(n);
    }
    (c, None)
}

/// Pide algo al programa `fondo` por `ENV:Escritorio/fondo-pide` (con la hora, para que cada vez cambie).
fn pedir(o: &mut Orden, que: &str) -> u64 {
    let texto = alloc::format!("# Lo que pide la orden Fondo al fondo de escritorio (se escribe y se lee, no se guarda).\npide = {} {}\n", que, dashos::ahora_us());
    match super::ajuste::escribir(o, "ENV", PIDE, texto.as_bytes()) {
        Ok(_) => o.fin(shell::RC_BIEN),
        Err(porque) => {
            o.error(format_args!("{}", porque));
            o.fin(shell::RC_FALLO)
        }
    }
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::FONDO);
    let que = args.texto("QUE").map(String::from);
    let valor = args.texto("VALOR").map(String::from);
    let pantalla = args.texto("PANTALLA").map(String::from);
    let (oscuro, claro, guardar) = (args.si("OSCURO"), args.si("CLARO"), args.si("GUARDAR"));
    let que = match que {
        None => return pedir(o, "abrir"),
        Some(q) => q,
    };
    let (mut c, antes) = leer_config(o);
    match que.to_ascii_uppercase().as_str() {
        "ABRIR" => return pedir(o, "abrir"),
        "CERRAR" => return pedir(o, "cerrar"),
        "SIGUIENTE" => return pedir(o, "siguiente"),
        "LISTA" => {
            for carpeta in ["Home:Fondos", "SYS:Fondos"] {
                match o.s.listar(carpeta) {
                    Ok(v) => {
                        let pngs: Vec<String> = v.into_iter().filter(|e| e.nombre.to_ascii_lowercase().ends_with(".png")).map(|e| e.nombre).collect();
                        di!(o, "{}: {}\n", carpeta, if pngs.is_empty() { String::from("(ninguno)") } else { pngs.join(", ") });
                    }
                    Err(e) => di!(o, "{}: {}\n", carpeta, crate::texto_error(e)),
                }
            }
            let animados: Vec<String> = animado::NOMBRES.iter().map(|n| alloc::format!("animado:{}", n)).collect();
            di!(o, "Animados: {}\n", animados.join(", "));
            di!(o, "En uso: {} (oscuro: {}); ajuste {}, transición {} de {} ms, pase {}, efecto {}, CPU {} %\n", c.fondo, c.oscuro.as_deref().unwrap_or("el mismo"), c.ajuste.nombre(), c.transicion, c.duracion, if c.pase == 0 { String::from("no") } else { alloc::format!("cada {} min", c.pase) }, c.efecto.as_deref().unwrap_or("ninguno"), c.cpu / 10);
            for (s, claro, osc) in &c.pantallas {
                di!(o, "  {}: {} (oscuro: {})\n", s, claro.as_deref().unwrap_or("el de todas"), osc.as_deref().unwrap_or("el mismo"));
            }
            return o.fin(shell::RC_BIEN);
        }
        clave @ ("AJUSTE" | "TRANSICION" | "PASE" | "EFECTO" | "CPU" | "DURACION") => {
            let Some(v) = valor else {
                o.error(format_args!("{} necesita un valor", clave));
                return o.fin(shell::RC_FALLO);
            };
            // Se comprueba como lo leerá `fondo`: un texto de una línea con esa clave.
            let linea = alloc::format!("{} = {}\n", clave.to_ascii_lowercase(), v);
            let (prueba, avisos) = ajustes::de_texto(linea.as_bytes());
            if let Some(a) = avisos.first() {
                o.error(format_args!("{}", a));
                return o.fin(shell::RC_FALLO);
            }
            match clave {
                "AJUSTE" => c.ajuste = Ajuste::de_nombre(&v).unwrap_or(prueba.ajuste),
                "TRANSICION" => c.transicion = prueba.transicion,
                "PASE" => c.pase = prueba.pase,
                "EFECTO" => c.efecto = prueba.efecto,
                "CPU" => c.cpu = prueba.cpu,
                _ => c.duracion = prueba.duracion,
            }
        }
        _ => {
            if !ajustes::nombre_vale(&que) {
                o.error(format_args!("{:?} no vale como fondo (Fondo LISTA dice los que hay)", que));
                return o.fin(shell::RC_FALLO);
            }
            let camino = ajustes::camino(&que);
            // Desde el Shell, `Home:` es el de la persona; el fondo de escritorio lee el suyo (hoy, el cajón `C` del
            // disco, como el Workbench): si aquí no se ve, se avisa y se pone igual (si no está, `fondo` lo dice y se
            // queda el de antes).
            if !camino.starts_with("animado:") && o.s.mirar(&camino).is_err() {
                di!(o, "Aviso: {} no se ve desde aquí; se pone igual\n", camino);
            }
            // Sin CLARO ni OSCURO, para los dos modos; con uno, solo para ese.
            if oscuro || claro {
                c.poner(pantalla.as_deref(), oscuro, &camino);
            } else {
                c.poner_los_dos(pantalla.as_deref(), &camino);
            }
        }
    }
    let texto = c.a_texto(antes.as_deref());
    for asignacion in if guardar { &["ENV", "ENVARC"][..] } else { &["ENV"][..] } {
        if let Err(porque) = super::ajuste::escribir(o, asignacion, ajustes::FICHERO, &texto) {
            o.error(format_args!("{}", porque));
            return o.fin(shell::RC_FALLO);
        }
    }
    di!(o, "Fondo: {}{}\n", c.elegir(pantalla.as_deref().unwrap_or(""), oscuro), if guardar { " (guardado)" } else { "" });
    o.fin(shell::RC_BIEN)
}
