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
    repositorio_con(m, k, serie, ps, &|fichero, _| vec![fichero.to_string()]);
}

/// Como `repositorio`, con las `Url` de cada paquete que diga `urls` (de su fichero en el repositorio y su nombre).
fn repositorio_con(m: &mut Memoria, k: &SigningKey, serie: u64, ps: &[Hecho], urls: &dyn Fn(&str, &str) -> Vec<String>) {
    let mut ix = format!("PKG-INDEX 2\nRepository: prueba\nArchitecture: aarch64-dashos\nSerial: {serie}\nExpires: 2030-01-01T00:00:00Z\n");
    for (i, h) in ps.iter().enumerate() {
        let a = archivo(h);
        let fichero = format!("{}-{}.tar.gz", h.nombre, h.version);
        m.poner(&format!("{REPO}/aarch64-dashos/{fichero}"), Some(&a));
        // Los campos del manifiesto que van al índice (los de dependencias y asignaciones), y los del índice.
        let de_mas: String = h.campos.lines().filter(|l| !l.starts_with("Permissions")).map(|l| format!("{l}\n")).collect();
        ix += &format!(
            "\nName: {}\nVersion: {}\nId: {}\n{}Size: {}\nSha256: {}\nInstall: drawer {}\n{de_mas}",
            h.nombre,
            h.version,
            i + 1,
            urls(&fichero, h.nombre).iter().map(|u| format!("Url: {u}\n")).collect::<String>(),
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

pub(crate) fn disco() -> Memoria {
    let mut m = Memoria::nueva(&["SYS:", "Paquetes:"]);
    m.solo_leer.push("SYS:".into());
    repositorio(&mut m, &clave(1), 5, &normales());
    m
}

pub(crate) struct Paq {
    pub(crate) nivel: Nivel,
}

impl Paq {
    pub(crate) fn nuevo() -> Paq {
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

    pub(crate) fn instalar(&self, m: &mut Memoria, nombres: &[&str]) -> Result<u32, Fallo> {
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

/// El repositorio de pruebas de verdad (el que va al disco de QEMU, `tools/repo_paq.py`, con el `tarfile` de Python
/// y la firma de `cryptography`), leído con lo de aquí: el contrato entre los dos, no un remedo. Sin Python con
/// `cryptography`, no se prueba (lo dice).
#[test]
fn repositorio_de_python() {
    use std::path::Path;
    let dir = std::env::temp_dir().join(format!("paq-repo-{}", std::process::id()));
    let guion = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/repo_paq.py");
    let Ok(s) = std::process::Command::new("python3").arg(&guion).arg(&dir).status() else {
        std::eprintln!("sin python3: no se prueba el repositorio de Python");
        return;
    };
    if !s.success() {
        std::eprintln!("tools/repo_paq.py no salió (¿falta cryptography?): no se prueba");
        return;
    }
    let mut m = Memoria::nueva(&["SYS:", "Paquetes:"]);
    m.solo_leer.push("SYS:".into());
    fn meter(m: &mut Memoria, base: &Path, rel: &str) {
        for e in std::fs::read_dir(base.join(rel)).unwrap() {
            let e = e.unwrap();
            let r = if rel.is_empty() { e.file_name().into_string().unwrap() } else { format!("{rel}/{}", e.file_name().into_string().unwrap()) };
            if e.file_type().unwrap().is_dir() {
                meter(m, base, &r);
            } else {
                m.poner(&format!("SYS:{r}"), Some(&std::fs::read(e.path()).unwrap()));
            }
        }
    }
    meter(&mut m, &dir, "");
    let _ = std::fs::remove_dir_all(&dir);
    let rs = cargar_repos(&mut m, &[]).unwrap();
    assert_eq!(rs.fuentes.len(), 1, "{:?}", rs.malos);
    assert!(rs.malos.iter().any(|(r, por)| r == "firmamal" && por.contains("firma")), "{:?}", rs.malos);
    let p = Paq::nuevo();
    for (n, que) in [("fuera", "se sale"), ("absoluto", "se sale"), ("enlace", "enlace simbólico"), ("vecino", "fuera de su cajón")] {
        let r = p.instalar(&mut m, &[n]);
        assert!(matches!(&r, Err(Fallo::Paquete(_, por)) if por.contains(que)), "{n}: {r:?}");
    }
    assert!(matches!(p.instalar(&mut m, &["cambiado"]), Err(Fallo::Huella(_))));
    assert!(matches!(p.instalar(&mut m, &["ajeno"]), Err(Fallo::NoEsta(_))));
    assert!(matches!(p.instalar(&mut m, &["imposible"]), Err(Fallo::Plan(_))));
    assert!(a_la_vista(&mut m).is_empty());
    assert_eq!(p.instalar(&mut m, &["saludo"]), Ok(1));
    assert_eq!(leeme(&m, "Saludo"), "saludo 1.0\n");
    assert_eq!(m.ver("Paquetes:Saludo/Saludo").unwrap().modo, 0o755);
    // La serie 2, copiada encima (como hace la prueba del Shell): Actualizar sube a la 1.1.
    for f in ["index.gz", "index.gz.sig"] {
        let d = m.ver(&format!("SYS:Work/Repos/prueba-2/{f}")).unwrap().datos.clone();
        m.poner(&format!("SYS:Work/Repos/prueba/aarch64-dashos/{f}"), Some(&d));
    }
    let nivel = p.nivel.clone();
    p.hacer(&mut m, |e, rs| Ok((plan_actualizar(e, rs, &Maquina::default(), &nivel)?, "Actualizar".into()))).unwrap();
    assert_eq!(leeme(&m, "Saludo"), "saludo 1.1\n");
    assert_eq!(p.instalar(&mut m, &["circulo-a"]), Ok(3));
}

/// `Url` repetida (formato 2, sección 5): se prueban en orden hasta que una da el `Size` y el `Sha256` del índice. Las
/// copias malas (otro tamaño, otra suma, una enorme, de la red, con un esquema raro, de un volumen, que se sale, que no
/// está) se tiran sin tocar nada, y se instala la buena, que va la última y escrita entera (como `repositorio.py`).
const LETRAS_MALAS: [&str; 7] = ["malas/tamano.tar.gz", "malas/suma.tar.gz", "malas/otra.tar.gz", "malas/enorme.tar.gz", "https://espejo.example/letras.tar.gz", "ftp://espejo.example/letras.tar.gz", "noesta.tar.gz"];

/// Las copias malas de `letras`: la buena cortada, la buena con un bit cambiado, otro paquete que también vale (letras
/// 6.6, con su LEEME y algo más grande) y uno enorme. Si se instalase una por error, se vería en su LEEME.
fn poner_copias_malas(m: &mut Memoria) {
    let buena = m.ver(&format!("{REPO}/aarch64-dashos/letras-1.0.tar.gz")).unwrap().datos.clone();
    let mut h = paquete("letras", "6.6", "Letras", "");
    h.ficheros.push(("Letras/Relleno".into(), vec![b'x'; 2000]));
    let otra = archivo(&h);
    assert_ne!(otra.len(), buena.len());
    let mut suma = buena.clone();
    let i = suma.len() / 2;
    suma[i] ^= 1;
    let enorme: Vec<u8> = otra.iter().cycle().take(otra.len() * 1000).copied().collect();
    m.poner(&format!("{REPO}/aarch64-dashos/malas/tamano.tar.gz"), Some(&buena[..buena.len() - 512]));
    m.poner(&format!("{REPO}/aarch64-dashos/malas/otra.tar.gz"), Some(&otra));
    m.poner(&format!("{REPO}/aarch64-dashos/malas/suma.tar.gz"), Some(&suma));
    m.poner(&format!("{REPO}/aarch64-dashos/malas/enorme.tar.gz"), Some(&enorme));
}

fn disco_con_copias(letras: &dyn Fn(&str) -> Vec<String>) -> Memoria {
    let mut m = Memoria::nueva(&["SYS:", "Paquetes:"]);
    m.solo_leer.push("SYS:".into());
    repositorio_con(&mut m, &clave(1), 5, &normales(), &|f, n| if n == "letras" { letras(f) } else { vec![f.to_string()] });
    poner_copias_malas(&mut m);
    m
}

fn nada_de_letras(m: &mut Memoria) {
    assert!(a_la_vista(m).is_empty());
    assert_eq!(cargar(m, &Nivel::de_todos()).unwrap().generaciones.len(), 0);
    assert!(m.listar("Paquetes:Paq/Almacen/letras").map_or(true, |v| v.is_empty()), "{:?}", m.listar("Paquetes:Paq/Almacen/letras"));
}

#[test]
fn copias_en_orden_hasta_la_buena() {
    let mut m = disco_con_copias(&|f| {
        let mut v: Vec<String> = LETRAS_MALAS.iter().map(|x| x.to_string()).collect();
        v.extend(["SYS:Estado/Paq/Repositorios", "../prueba/aarch64-dashos/letras-1.0.tar.gz", "javascript:alert(1)", "file:///etc/passwd", "ñññññññññññññññññ://x"].map(String::from));
        v.push(format!("{REPO}/aarch64-dashos/{f}"));
        v
    });
    assert_eq!(Paq::nuevo().instalar(&mut m, &["letras"]), Ok(1));
    assert_eq!(leeme(&m, "Letras"), "letras 1.0\n");
    let e = cargar(&mut m, &Nivel::de_todos()).unwrap();
    assert_eq!(e.almacen.len(), 1);
    assert_eq!(e.almacen[0].ficha.sha256, huella::sha256(&m.ver(&format!("{REPO}/aarch64-dashos/letras-1.0.tar.gz")).unwrap().datos));
    // La primera buena gana: con la buena delante, las malas de detrás ni se miran (y da lo mismo que estén).
    let mut m = disco_con_copias(&|f| core::iter::once(f.to_string()).chain(LETRAS_MALAS.iter().map(|x| x.to_string())).collect());
    assert_eq!(Paq::nuevo().instalar(&mut m, &["letras"]), Ok(1));
    assert_eq!(leeme(&m, "Letras"), "letras 1.0\n");
}

#[test]
fn todas_las_copias_malas() {
    let mut m = disco_con_copias(&|_| LETRAS_MALAS.iter().map(|x| x.to_string()).collect());
    let r = Paq::nuevo().instalar(&mut m, &["letras"]);
    let Err(Fallo::Huella(q)) = &r else { panic!("{r:?}") };
    for x in ["letras 1.0 (malas/tamano.tar.gz: da otro tamaño; ", "malas/suma.tar.gz: da otra suma", "malas/otra.tar.gz: da otro tamaño",
        "malas/enorme.tar.gz: da otro tamaño", "https://espejo.example/letras.tar.gz: es de la red", "ftp://espejo.example/letras.tar.gz: se sale del repositorio",
        "noesta.tar.gz: no se puede leer)"]
    {
        assert!(q.contains(x), "falta «{x}» en: {q}");
    }
    nada_de_letras(&mut m);
    // Una sola dirección mala: lo de siempre, sin la lista.
    let mut m = disco_con_copias(&|_| vec!["malas/suma.tar.gz".into()]);
    assert_eq!(Paq::nuevo().instalar(&mut m, &["letras"]), Err(Fallo::Huella("letras 1.0".into())));
    nada_de_letras(&mut m);
}

#[test]
fn ninguna_copia_se_puede_bajar() {
    let mut m = disco_con_copias(&|_| vec!["https://espejo.example/l.tar.gz".into(), "SYS:Estado/Paq/Repositorios".into(), "noesta.tar.gz".into()]);
    let r = Paq::nuevo().instalar(&mut m, &["letras"]);
    assert!(matches!(&r, Err(Fallo::Paquete(n, por)) if n == "letras" && por.starts_with("https://espejo.example/l.tar.gz: es de la red") && por.contains("se sale del repositorio")), "{r:?}");
    nada_de_letras(&mut m);
}

#[test]
fn mil_copias_o_un_tamano_enorme() {
    // Mil direcciones: la entrada no se usa (las demás del índice, sí).
    let mut m = disco_con_copias(&|f| (0..1000).map(|i| if i == 999 { f.to_string() } else { format!("c{i}.tar.gz") }).collect());
    let rs = cargar_repos(&mut m, &[]).unwrap();
    assert_eq!(rs.fuentes[0].indice.malas, 1);
    assert!(rs.fuentes[0].indice.entradas.iter().all(|e| e.nombre != "letras"));
    assert_eq!(Paq::nuevo().instalar(&mut m, &["letras"]), Err(Fallo::NoEsta("letras".into())));
    nada_de_letras(&mut m);
    // Un `Size` más grande de lo que cabe: no se lee más de `MAX_PAQUETE`, y lo leído es "otro tamaño".
    let mut m = disco();
    let mut rs = cargar_repos(&mut m, &[]).unwrap();
    let x = rs.fuentes[0].indice.entradas.iter_mut().find(|e| e.nombre == "letras").unwrap();
    x.tamano = 1 << 40;
    let x = x.clone();
    let r = traer(&mut m, &Nivel::de_todos(), &rs.fuentes[0], &x);
    assert!(matches!(&r, Err(Fallo::Huella(q)) if q == "letras 1.0"), "{:?}", r.map(|_| ()));
}
