//! Las pruebas del motor, con un repositorio y un disco en memoria: instalar, actualizar, volver, quitar y limpiar, y lo
//! hostil (índices y paquetes manipulados, rutas que se salen, enlaces, dependencias en círculo o imposibles, dos Paq
//! a la vez, cortes de luz en cada paso, tablas dañadas).

use super::*;
use crate::disco::memoria::Memoria;
use crate::firma::pruebas::{clave, firmar};
use alloc::borrow::ToOwned;
use alloc::string::ToString;
use ed25519_dalek::SigningKey;

const REPO: &str = "SYS:Repos/prueba";

/// Un paquete de prueba: su manifiesto (campos de más) y sus ficheros (ruta, bytes). Con `tar_a_mano`, el tar entero.
struct Hecho {
    nombre: &'static str,
    version: &'static str,
    cajon: &'static str,
    campos: &'static str,
    ficheros: Vec<(String, Vec<u8>)>,
    tar_a_mano: Option<Vec<u8>>,
}

fn paquete(nombre: &'static str, version: &'static str, cajon: &'static str, campos: &'static str) -> Hecho {
    Hecho {
        nombre,
        version,
        cajon,
        campos,
        ficheros: vec![(format!("{cajon}/LEEME"), format!("{nombre} {version}\n").into_bytes()), (format!("{cajon}/C/{nombre}"), b"Echo hola\n".to_vec())],
        tar_a_mano: None,
    }
}

fn readme(h: &Hecho) -> String {
    format!("Format: 2\nName: {}\nVersion: {}\nArchitecture: aarch64-dashos\nInstall: drawer {}\nShort: prueba\n{}\nUn paquete de prueba.\n", h.nombre, h.version, h.cajon, h.campos)
}

fn archivo(h: &Hecho) -> Vec<u8> {
    let tar = h.tar_a_mano.clone().unwrap_or_else(|| {
        let r = readme(h);
        let mut es: Vec<(&str, bool, u16, &[u8])> = vec![(h.cajon, true, 0o755, b"")];
        for (ruta, b) in &h.ficheros {
            es.push((ruta, false, 0o755, b));
        }
        let nombre_readme = format!("{}.readme", h.nombre);
        es.push((&nombre_readme, false, 0o644, r.as_bytes()));
        tar::escribir(&es)
    });
    gz::sin_comprimir(&tar)
}

/// Pone el repositorio en `m`: los paquetes, su índice firmado con `k` y la lista y la clave en `SYS:Estado/Paq`.
fn repositorio(m: &mut Memoria, k: &SigningKey, serie: u64, ps: &[Hecho]) {
    let mut ix = format!("PKG-INDEX 2\nRepository: prueba\nArchitecture: aarch64-dashos\nSerial: {serie}\nExpires: 2030-01-01T00:00:00Z\n");
    for (i, h) in ps.iter().enumerate() {
        let a = archivo(h);
        let fichero = format!("{}-{}.tar.gz", h.nombre, h.version);
        m.poner(&format!("{REPO}/aarch64-dashos/{fichero}"), Some(&a));
        // Los campos del manifiesto que van al índice (los de dependencias y asignaciones), y los del índice.
        let de_mas: String = h.campos.lines().filter(|l| !l.starts_with("Permissions")).map(|l| format!("{l}\n")).collect();
        ix += &format!(
            "\nName: {}\nVersion: {}\nId: {}\nUrl: {fichero}\nSize: {}\nSha256: {}\nInstall: drawer {}\n{de_mas}",
            h.nombre,
            h.version,
            i + 1,
            a.len(),
            huella::hex(&huella::sha256(&a)),
            h.cajon
        );
    }
    let gz = gz::sin_comprimir(ix.as_bytes());
    m.poner(&format!("{REPO}/aarch64-dashos/index.gz.sig"), Some(firmar(k, firma::INDICE, &gz).as_bytes()));
    m.poner(&format!("{REPO}/aarch64-dashos/index.gz"), Some(&gz));
    m.poner(config::REPOSITORIOS, Some(format!("Repository: prueba\nUrl: {REPO}\nPriority: 20\n").as_bytes()));
    let publica = clave(1).verifying_key().to_bytes();
    m.poner(&format!("{}/prueba", config::CLAVES), Some(format!("# La de pruebas\nRepository: prueba\nUse: index\nKey: {}\n", huella::hex(&publica)).as_bytes()));
}

fn normales() -> Vec<Hecho> {
    vec![
        paquete("saludo", "1.0", "Saludo", "Requires: pkg:letras >= 1.0\nAssigns: SALUDO: Saludo"),
        paquete("saludo", "1.1", "Saludo", "Requires: pkg:letras >= 1.0\nAssigns: SALUDO: Saludo"),
        paquete("letras", "1.0", "Letras", ""),
        paquete("circulo-a", "1.0", "CirculoA", "Requires: pkg:circulo-b"),
        paquete("circulo-b", "1.0", "CirculoB", "Requires: pkg:circulo-a"),
        paquete("imposible", "1.0", "Imposible", "Requires: pkg:noexiste"),
        paquete("choque", "1.0", "Choque", "Conflicts: pkg:saludo"),
    ]
}

fn disco() -> Memoria {
    let mut m = Memoria::nueva(&["SYS:", "Paquetes:"]);
    m.solo_leer.push("SYS:".into());
    repositorio(&mut m, &clave(1), 5, &normales());
    m
}

struct Paq {
    nivel: Nivel,
}

impl Paq {
    fn nuevo() -> Paq {
        Paq { nivel: Nivel::de_todos() }
    }

    /// Una transacción entera, como la hace el servicio: cerrojo, en orden, repositorios, plan y aplicar.
    fn hacer(&self, m: &mut Memoria, plan: impl FnOnce(&Estado, &Repos) -> Result<(Propuesta, String), Fallo>) -> Result<u32, Fallo> {
        cerrar(m, &self.nivel, "prueba")?;
        let r = (|| {
            let (mut e, _) = poner_en_orden(m, &self.nivel)?;
            let vistas = leer_series(m, &self.nivel);
            let rs = cargar_repos(m, &vistas)?;
            let (p, que) = plan(&e, &rs)?;
            let n = aplicar(m, &self.nivel, &mut e, &rs, &Maquina::default(), &p.cambios, &p.pedidos, &que)?;
            guardar_series(m, &self.nivel, &vistas, &rs.series)?;
            limpiar(m, &self.nivel, &mut e, Conservar::Solo)?;
            Ok(n)
        })();
        abrir(m, &self.nivel);
        r
    }

    fn instalar(&self, m: &mut Memoria, nombres: &[&str]) -> Result<u32, Fallo> {
        let nivel = self.nivel.clone();
        let pedidos: Vec<String> = nombres.iter().map(|x| x.to_string()).collect();
        self.hacer(m, |e, rs| Ok((plan_instalar(e, rs, &Maquina::default(), &nivel, &pedidos)?, format!("Instalar {}", pedidos.join(", ")))))
    }
}

fn a_la_vista(m: &mut Memoria) -> Vec<String> {
    let mut v: Vec<String> = m.listar("Paquetes:").unwrap().into_iter().map(|x| x.0).filter(|x| x != "Paq").collect();
    v.sort();
    v
}

fn leeme(m: &Memoria, cajon: &str) -> String {
    String::from_utf8(m.ver(&format!("Paquetes:{cajon}/LEEME")).unwrap().datos.clone()).unwrap()
}

#[test]
fn instalar_actualizar_volver_quitar_y_limpiar() {
    let mut m = disco();
    let p = Paq::nuevo();
    // Sin pedir versión: la más nueva, con su dependencia.
    assert_eq!(p.instalar(&mut m, &["saludo"]), Ok(1));
    assert_eq!(a_la_vista(&mut m), ["Letras", "Saludo"]);
    assert_eq!(leeme(&m, "Saludo"), "saludo 1.1\n");
    let a = String::from_utf8(m.ver("Paquetes:Paq/Asignaciones").unwrap().datos.clone()).unwrap();
    assert!(a.contains("SALUDO=Paquetes:Saludo\n"), "{a}");
    let e = cargar(&mut m, &p.nivel).unwrap();
    assert_eq!(e.instalado("letras").unwrap().motivo, Motivo::Dependencia);
    assert_eq!(e.instalado("saludo").unwrap().motivo, Motivo::Pedido);
    // Lo mismo otra vez: nada que hacer.
    assert!(matches!(p.instalar(&mut m, &["saludo"]), Err(Fallo::Plan(_))));

    // Volver a la de antes de la 1 no hay; volver tras bajar a 1.0 a mano no se puede (no está en el almacén).
    let nivel = p.nivel.clone();
    assert!(matches!(p.hacer(&mut m, |e, _| plan_volver(e, &nivel, None).map(|x| (x.0, String::new()))), Err(Fallo::Plan(_))));

    // Quitar saludo se lleva letras (nadie más la necesita).
    let n2 = p.hacer(&mut m, |e, _| Ok((plan_quitar(e, &nivel, &["saludo".into()])?, "Quitar saludo".into()))).unwrap();
    assert_eq!(n2, 2);
    assert!(a_la_vista(&mut m).is_empty());
    // Deshacer la última: vuelven los dos, del almacén (sin bajar nada).
    m.nodos.retain(|k, _| !k.starts_with("sys:repos/prueba/aarch64-dashos/saludo"));
    let n3 = p.hacer(&mut m, |e, _| plan_volver(e, &nivel, None).map(|(x, a)| (x, format!("Volver a la {a}")))).unwrap();
    assert_eq!(n3, 3);
    assert_eq!(a_la_vista(&mut m), ["Letras", "Saludo"]);
    assert_eq!(leeme(&m, "Saludo"), "saludo 1.1\n");
    let e = cargar(&mut m, &p.nivel).unwrap();
    assert_eq!(e.generaciones.iter().map(|g| g.numero).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(e.generacion.cambio, "Volver a la 1");
}

#[test]
fn actualizar_y_volver_atras() {
    let mut m = Memoria::nueva(&["SYS:", "Paquetes:"]);
    let mut ps = normales();
    ps.remove(1);
    repositorio(&mut m, &clave(1), 5, &ps);
    let p = Paq::nuevo();
    assert_eq!(p.instalar(&mut m, &["saludo"]), Ok(1));
    assert_eq!(leeme(&m, "Saludo"), "saludo 1.0\n");
    // Sale la 1.1: Actualizar la sube.
    repositorio(&mut m, &clave(1), 6, &normales());
    let nivel = p.nivel.clone();
    let n = p.hacer(&mut m, |e, rs| Ok((plan_actualizar(e, rs, &Maquina::default(), &nivel)?, "Actualizar".into()))).unwrap();
    assert_eq!(n, 2);
    assert_eq!(leeme(&m, "Saludo"), "saludo 1.1\n");
    // Un programa "abierto" con la 1.1 sigue con sus ficheros: el cajón se mueve entero, no se pisa.
    // Volver a la 1: la 1.0, del almacén.
    let e = cargar(&mut m, &nivel).unwrap();
    let (pr, _) = plan_volver(&e, &nivel, Some(1)).unwrap();
    assert_eq!(pr.cambios.len(), 1);
    assert!(matches!(&pr.cambios[0], Cambio::Volver { .. }));
    p.hacer(&mut m, |e, _| plan_volver(e, &nivel, Some(1)).map(|(x, _)| (x, "Volver a la 1".into()))).unwrap();
    assert_eq!(leeme(&m, "Saludo"), "saludo 1.0\n");
    // La 1.1 sigue en el almacén (la 2 aún se guarda); Limpiar con "solo la de ahora" se la lleva.
    let mut e = cargar(&mut m, &nivel).unwrap();
    assert!(e.guardada("saludo", &texto::leer_version("1.1-1").unwrap()).is_some());
    let l = limpiar(&mut m, &nivel, &mut e, Conservar::Actual).unwrap();
    assert_eq!((l.generaciones, l.versiones), (2, 1));
    let e = cargar(&mut m, &nivel).unwrap();
    assert!(e.guardada("saludo", &texto::leer_version("1.1-1").unwrap()).is_none());
    assert_eq!(e.generaciones.len(), 1);
    assert_eq!(a_la_vista(&mut m), ["Letras", "Saludo"]);
}

#[test]
fn indice_con_firma_mala_o_viejo() {
    let mut m = disco();
    // Firmado con otra clave: el repositorio no se usa.
    repositorio(&mut m, &clave(2), 5, &normales());
    let rs = cargar_repos(&mut m, &[]).unwrap();
    assert!(rs.fuentes.is_empty());
    assert!(rs.malos[0].1.contains("clave que no es de confianza"), "{:?}", rs.malos);
    // Bien firmado y cambiado después.
    let mut m = disco();
    let r = format!("{REPO}/aarch64-dashos/index.gz");
    let mut ix = m.ver(&r).unwrap().datos.clone();
    let i = ix.len() / 2;
    ix[i] ^= 1;
    m.poner(&r, Some(&ix));
    let rs = cargar_repos(&mut m, &[]).unwrap();
    assert!(rs.fuentes.is_empty() && rs.malos[0].1.contains("firma de su índice no vale"));
    assert!(matches!(Paq::nuevo().instalar(&mut m, &["saludo"]), Err(Fallo::NoEsta(_))));
    assert!(a_la_vista(&mut m).is_empty());
    // Uno bueno pero más viejo que el último visto.
    let mut m = disco();
    let rs = cargar_repos(&mut m, &[("prueba".into(), 9)]).unwrap();
    assert!(rs.malos[0].1.contains("más viejo"));
}

#[test]
fn paquete_cambiado_tras_firmar() {
    let mut m = disco();
    let r = format!("{REPO}/aarch64-dashos/letras-1.0.tar.gz");
    let mut a = m.ver(&r).unwrap().datos.clone();
    let i = a.len() - 20;
    a[i] ^= 0x20;
    m.poner(&r, Some(&a));
    assert!(matches!(Paq::nuevo().instalar(&mut m, &["saludo"]), Err(Fallo::Huella(_))));
    // Nada a la vista ni ninguna generación; en el almacén, a lo sumo lo ya comprobado.
    assert!(a_la_vista(&mut m).is_empty());
    assert_eq!(cargar(&mut m, &Nivel::de_todos()).unwrap().generaciones.len(), 0);
}

fn con_tar(nombre: &'static str, cajon: &'static str, entradas: &[(&str, bool, u16, &[u8])]) -> Hecho {
    let mut h = paquete(nombre, "1.0", cajon, "");
    let r = readme(&h);
    let leeme = format!("{nombre}.readme");
    let mut es: Vec<(&str, bool, u16, &[u8])> = entradas.to_vec();
    es.push((&leeme, false, 0o644, r.as_bytes()));
    h.tar_a_mano = Some(tar::escribir(&es));
    h
}

#[test]
fn paquetes_que_se_salen() {
    let mut casos = vec![
        con_tar("fuera", "Fuera", &[("Fuera", true, 0o755, b""), ("../Fuera.txt", false, 0o644, b"x")]),
        con_tar("absoluto", "Absoluto", &[("Absoluto", true, 0o755, b""), ("/C/Absoluto", false, 0o644, b"x")]),
        con_tar("volumen", "Volumen", &[("SYS:C/Volumen", false, 0o644, b"x")]),
        con_tar("vecino", "Vecino", &[("Vecino", true, 0o755, b""), ("Saludo/LEEME", false, 0o644, b"te piso")]),
    ];
    let mut enlace = con_tar("enlace", "Enlace", &[("Enlace", true, 0o755, b""), ("Enlace/a", false, 0o644, b"")]);
    // El segundo bloque del tar es la cabecera de "Enlace/a": a enlace simbólico.
    let t = enlace.tar_a_mano.as_mut().unwrap();
    t[512 + 156] = b'2';
    t[512 + 148..512 + 156].copy_from_slice(b"        ");
    let s: u64 = t[512..1024].iter().map(|&b| b as u64).sum();
    t[512 + 148..512 + 156].copy_from_slice(format!("{:06o}\0 ", s).as_bytes());
    casos.push(enlace);
    casos.push(paquete("sistema", "1.0", "Sistema", "Assigns: LIBS: Sistema"));
    casos.push(paquete("asignafuera", "1.0", "AsignaFuera", "Assigns: FUERA: Saludo"));
    let nombres: Vec<&str> = casos.iter().map(|h| h.nombre).collect();
    let mut m = Memoria::nueva(&["SYS:", "Paquetes:"]);
    let mut todos = normales();
    todos.extend(casos);
    repositorio(&mut m, &clave(1), 5, &todos);
    for n in nombres {
        let r = Paq::nuevo().instalar(&mut m, &[n]);
        assert!(matches!(r, Err(Fallo::Paquete(..))), "{n}: {r:?}");
        assert!(a_la_vista(&mut m).is_empty(), "{n}");
        assert!(m.ver("Paquetes:Fuera.txt").is_none() && m.ver("SYS:C/Volumen").is_none());
    }
    // Lo que dejó a medias se barre en la siguiente.
    let (_, a) = poner_en_orden(&mut m, &Nivel::de_todos()).unwrap();
    assert_eq!(a.borrados, 0);
}

#[test]
fn dependencias_en_circulo_imposibles_y_choques() {
    let mut m = disco();
    let p = Paq::nuevo();
    assert_eq!(p.instalar(&mut m, &["circulo-a"]), Ok(1));
    assert_eq!(a_la_vista(&mut m), ["CirculoA", "CirculoB"]);
    let r = p.instalar(&mut m, &["imposible"]);
    assert!(matches!(&r, Err(Fallo::Plan(x)) if x[0].contains("noexiste")), "{r:?}");
    assert!(matches!(p.instalar(&mut m, &["noexiste"]), Err(Fallo::NoEsta(_))));
    assert_eq!(p.instalar(&mut m, &["saludo"]), Ok(2));
    let r = p.instalar(&mut m, &["choque"]);
    assert!(matches!(&r, Err(Fallo::Plan(x)) if x[0].contains("choca")), "{r:?}");
    // Quitar letras rompería saludo.
    let nivel = p.nivel.clone();
    let r = p.hacer(&mut m, |e, _| Ok((plan_quitar(e, &nivel, &["letras".into()])?, String::new())));
    assert!(matches!(&r, Err(Fallo::Plan(x)) if x[0].contains("saludo")), "{r:?}");
    assert_eq!(a_la_vista(&mut m), ["CirculoA", "CirculoB", "Letras", "Saludo"]);
}

#[test]
fn dos_paq_a_la_vez() {
    let mut m = disco();
    let n = Nivel::de_todos();
    cerrar(&mut m, &n, "proceso 7").unwrap();
    let r = Paq::nuevo().instalar(&mut m, &["saludo"]);
    assert!(matches!(&r, Err(Fallo::Ocupado(q)) if q.contains("proceso 7")), "{r:?}");
    // Al acabar el otro, sí. (`hacer` soltó el cerrojo... del otro: aquí se vuelve a poner para ver que se respeta.)
    assert!(a_la_vista(&mut m).is_empty());
}

/// Un corte de luz en cada escritura de una transacción: después de arrancar otra vez (poner en orden), lo que se ve es
/// la generación de antes entera o la nueva entera.
#[test]
fn cortes_en_cada_paso() {
    let base = {
        let mut m = disco();
        let mut ps = normales();
        ps.remove(1);
        repositorio(&mut m, &clave(1), 5, &ps);
        Paq::nuevo().instalar(&mut m, &["saludo"]).unwrap();
        repositorio(&mut m, &clave(1), 6, &normales());
        m
    };
    let nivel = Nivel::de_todos();
    let actualizar = |m: &mut Memoria| Paq::nuevo().hacer(m, |e, rs| Ok((plan_actualizar(e, rs, &Maquina::default(), &nivel)?, "Actualizar".into())));
    let escrituras = {
        let mut m = base.clone();
        let antes = m.escrituras;
        actualizar(&mut m).unwrap();
        m.escrituras - antes
    };
    assert!(escrituras > 5);
    let mut viejas = 0;
    let mut nuevas = 0;
    for corte in 0..escrituras {
        let mut m = base.clone();
        m.corte = Some(m.escrituras + corte);
        // Si el corte cae en lo último (soltar el cerrojo), la transacción ya estaba hecha.
        let _ = actualizar(&mut m);
        // Vuelve la luz: el cerrojo se quedó (lo quita el servicio al arrancar) y se pone en orden.
        m.corte = None;
        abrir(&mut m, &nivel);
        let (e, _) = poner_en_orden(&mut m, &nivel).unwrap();
        let ve = leeme(&m, "Saludo");
        assert_eq!(a_la_vista(&mut m), ["Letras", "Saludo"], "corte {corte}");
        match e.generacion.numero {
            1 => {
                assert_eq!(ve, "saludo 1.0\n", "corte {corte}");
                viejas += 1;
            }
            2 => {
                assert_eq!(ve, "saludo 1.1\n", "corte {corte}");
                nuevas += 1;
            }
            x => panic!("generación {x}"),
        }
        // Y desde ahí se sigue: actualizar otra vez acaba en la 1.1.
        if e.generacion.numero == 1 {
            actualizar(&mut m).unwrap();
        }
        assert_eq!(leeme(&m, "Saludo"), "saludo 1.1\n");
        // Sin basura en el almacén.
        let (_, a) = poner_en_orden(&mut m, &nivel).unwrap();
        assert_eq!(a, Arreglo::default(), "corte {corte}");
    }
    assert!(viejas > 0 && nuevas > 0);
}

#[test]
fn tabla_danada() {
    let mut m = disco();
    let p = Paq::nuevo();
    p.instalar(&mut m, &["letras"]).unwrap();
    p.instalar(&mut m, &["circulo-a"]).unwrap();
    let r = "Paquetes:Paq/Generaciones/2";
    let mut t = m.ver(r).unwrap().datos.clone();
    t.truncate(t.len() - 4);
    m.poner(r, Some(&t));
    let (e, _) = poner_en_orden(&mut m, &p.nivel).unwrap();
    assert_eq!(e.generacion.numero, 1);
    assert!(e.avisos.iter().any(|a| a.contains("La generación 2 está dañada")), "{:?}", e.avisos);
    assert_eq!(a_la_vista(&mut m), ["Letras"]);
    // La siguiente transacción es la 3, no pisa la 2.
    assert_eq!(p.instalar(&mut m, &["saludo"]), Ok(3));
}

#[test]
fn cajon_ajeno_en_el_sitio() {
    // En Home:Paquetes, el usuario puede haber hecho un cajón con el nombre: no se pisa.
    let mut m = disco();
    m.poner("Paquetes:Letras/mio.txt", Some(b"mio"));
    let r = Paq::nuevo().instalar(&mut m, &["letras"]);
    assert!(matches!(&r, Err(Fallo::Danado(x)) if x.contains("no es de Paq")), "{r:?}");
    assert_eq!(m.ver("Paquetes:Letras/mio.txt").unwrap().datos, b"mio".to_owned());
}
