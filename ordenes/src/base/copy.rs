//! `Copy`: copia ficheros. `Copy origen|patrón... [TO] destino [ALL] [QUIET]`. Con varios orígenes, o si el destino es
//! un cajón, van dentro con su nombre. De un cajón se copian sus ficheros; con `ALL`, también sus cajones. Sobrescribe.
//! La copia lleva los permisos y los bits del original.

use alloc::string::String;
use alloc::vec::Vec;
use dashos::protocolo::archivos as a;
use crate::{di, Orden};

const MAX_NIVELES: usize = 64;

struct Cuenta {
    copiados: usize,
    fallos: usize,
}

/// Copia un fichero. El error, con si fue en el destino.
fn copiar_fichero(o: &mut Orden, de: &str, hacia: &str) -> Result<(), (i32, bool)> {
    let (asa, d) = o.s.abrir(de).map_err(|e| (e, false))?;
    let r = (|| {
        let (asa2, _) = o.s.abrir_para_escribir(hacia, false).map_err(|e| (e, true))?;
        let mut buf = alloc::vec![0u8; 32 * 1024];
        let mut hecho = 0u64;
        let r = loop {
            let n = match o.s.leer(asa, hecho, &mut buf) {
                Ok(0) => break Ok(()),
                Ok(n) => n,
                Err(e) => break Err((e, false)),
            };
            if let Err(e) = o.s.escribir(asa2, hecho, &buf[..n]) {
                break Err((e, true));
            }
            hecho += n as u64;
            o.pulso();
        };
        let cerrar = o.s.cerrar(asa2);
        r?;
        if cerrar != a::BIEN {
            return Err((cerrar, true));
        }
        // Los permisos y los bits del original (si no se puede, la copia vale igual).
        let _ = o.s.proteger(hacia, d.bits, d.modo & 0o7777);
        Ok(())
    })();
    o.s.cerrar(asa);
    r
}

#[allow(clippy::too_many_arguments)]
fn copiar(o: &mut Orden, de: &str, cajon: bool, hacia: &str, todo: bool, callado: bool, nivel: usize, c: &mut Cuenta) {
    if !cajon {
        match copiar_fichero(o, de, hacia) {
            Ok(()) => {
                c.copiados += 1;
                if !callado {
                    di!(o, "{}  copiado\n", shell::ruta::ultimo(de));
                }
            }
            Err((e, en_destino)) => {
                o.error_de(e, if en_destino { hacia } else { de });
                c.fallos += 1;
            }
        }
        return;
    }
    if nivel >= MAX_NIVELES {
        o.error(format_args!("{}: demasiados cajones unos dentro de otros", de));
        c.fallos += 1;
        return;
    }
    // Un cajón: su destino tiene que ser un cajón (se crea si falta), y dentro van sus ficheros.
    match o.s.mirar(hacia) {
        Ok(d) if d.cajon() => {}
        Ok(_) => {
            o.error_de(a::NO_ES_CAJON, hacia);
            c.fallos += 1;
            return;
        }
        Err(_) => match o.s.crear(hacia, a::CAJON) {
            Ok(asa) => {
                o.s.cerrar(asa);
            }
            Err(e) => {
                o.error_de(e, hacia);
                c.fallos += 1;
                return;
            }
        },
    }
    let entradas = match o.s.listar(de) {
        Ok(e) => e,
        Err(e) => {
            o.error_de(e, de);
            c.fallos += 1;
            return;
        }
    };
    for e in entradas {
        if e.datos.cajon() && !todo {
            continue;
        }
        let (hijo, destino) = (shell::ruta::unir(de, &e.nombre), shell::ruta::unir(hacia, &e.nombre));
        copiar(o, &hijo, e.datos.cajon(), &destino, todo, callado, nivel + 1, c);
    }
}

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir("FROM/M,TO/A,ALL/S,QUIET/S");
    let hacia = match o.s.ruta(args.texto("TO").unwrap_or("")) {
        Ok(r) => r,
        Err(e) => {
            o.error_de(e, args.texto("TO").unwrap_or(""));
            return o.fin(shell::RC_FALLO);
        }
    };
    if args.varios("FROM").is_empty() {
        o.error(format_args!("falta qué copiar"));
        return o.fin(shell::RC_FALLO);
    }
    let mut c = Cuenta { copiados: 0, fallos: 0 };
    let mut origenes: Vec<(String, bool)> = Vec::new();
    for f in args.varios("FROM") {
        match o.s.expandir(f) {
            Ok(h) if h.is_empty() => {
                o.error_de(a::NO_EXISTE, f);
                c.fallos += 1;
            }
            Ok(h) => origenes.extend(h.into_iter().map(|x| (x.ruta, x.cajon))),
            Err(e) => {
                o.error_de(e, f);
                c.fallos += 1;
            }
        }
    }
    let destino_cajon = o.s.mirar(&hacia).is_ok_and(|d| d.cajon());
    if origenes.len() > 1 && !destino_cajon {
        o.error(format_args!("{}: con varios orígenes, el destino tiene que ser un cajón", hacia));
        return o.fin(shell::RC_FALLO);
    }
    for (de, cajon) in origenes {
        // A un cajón, dentro y con su nombre; un cajón entero, sobre el cajón de destino.
        let destino = if destino_cajon && !cajon { shell::ruta::unir(&hacia, shell::ruta::ultimo(&de)) } else { hacia.clone() };
        copiar(o, &de, cajon, &destino, args.si("ALL"), args.si("QUIET"), 0, &mut c);
    }
    o.fin(if c.fallos > 0 { shell::RC_ERROR } else { shell::RC_BIEN })
}
