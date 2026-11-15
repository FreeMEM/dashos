//! Las pruebas de las asignaciones en el PC: `rustc --edition 2024 --test usuario/src/asignaciones.rs`. Lo del servicio,
//! contra uno falso en memoria con raíces (cada una con su tope), cajones que se pueden escribir o no, y la ventana de
//! `LISTAR`.

use super::*;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------------------------------------------
// La tabla y la resolución.

/// La tabla de una persona, como la monta `sesiones`: volúmenes, `PROGDIR` y las de la sesión.
fn de_persona() -> Tabla {
    let mut t = Tabla::vacia();
    t.poner_volumen(b"SYS", 1, b"").unwrap();
    t.poner_volumen(b"Home", 2, b"").unwrap();
    t.poner_volumen(b"RAM", 3, b"").unwrap();
    t.poner_volumen(b"Work", 4, b"").unwrap();
    t.poner_progdir(0).unwrap();
    de_sesion(&mut t, false).unwrap();
    t
}

fn sitios(t: &Tabla, c: &str) -> Result<Vec<(u8, String)>, Error> {
    let mut s = Sitios::nuevos();
    t.resolver(c.as_bytes(), &mut s)?;
    Ok((0..s.len()).map(|i| s.sitio(i).unwrap()).map(|(r, c)| (r, String::from_utf8(c.to_vec()).unwrap())).collect())
}

fn v(x: &[(u8, &str)]) -> Vec<(u8, String)> {
    x.iter().map(|&(r, c)| (r, c.to_string())).collect()
}

#[test]
fn volumenes_y_asignaciones() {
    let t = de_persona();
    assert_eq!(sitios(&t, "SYS:"), Ok(v(&[(1, "")])));
    assert_eq!(sitios(&t, "sys:Libs/x.library"), Ok(v(&[(1, "Libs/x.library")])));
    assert_eq!(sitios(&t, "Home:Documentos/"), Ok(v(&[(2, "Documentos")])));
    assert_eq!(sitios(&t, "PROGDIR:datos"), Ok(v(&[(0, "datos")])));
    // Multiasignación con capas: primero la del usuario.
    assert_eq!(sitios(&t, "C:List"), Ok(v(&[(2, "C/List"), (1, "C/List")])));
    assert_eq!(sitios(&t, "libs:"), Ok(v(&[(2, "Libs"), (1, "Libs")])));
    assert_eq!(sitios(&t, "ENVARC:Sys/locale.prefs"), Ok(v(&[(2, "Prefs/Env-Archive/Sys/locale.prefs"), (1, "Prefs/Env-Archive/Sys/locale.prefs")])));
    // Una asignación que lleva a otra: ENV: es RAM:Env.
    assert_eq!(sitios(&t, "ENV:Sys/font.prefs"), Ok(v(&[(3, "Env/Sys/font.prefs")])));
    assert_eq!(sitios(&t, "T:"), Ok(v(&[(3, "T")])));
    assert_eq!(sitios(&t, "KEYMAPS:es"), Ok(v(&[(1, "Devs/Keymaps/es")])));
    // Los volúmenes salen en su orden.
    let vols: Vec<&[u8]> = t.volumenes().map(|l| l.nombre).collect();
    assert_eq!(vols, [&b"SYS"[..], b"Home", b"RAM", b"Work"]);
}

#[test]
fn rutas_hostiles() {
    let t = de_persona();
    for malo in [
        "SYS", "", ":", ":x", "SYS::", "SYS:a:b", "C:x:y", "SYS:/", "SYS:/Libs", "SYS:..", "SYS:Libs/..", "SYS:Libs/../S", "SYS:.", "SYS:a//b",
        "SYS://", "Home:a/./b", "SY S:x", "SYS\n:x", "Ho/me:x", "SYS:a\u{1}b", "SYS:a\u{7f}",
    ] {
        assert_eq!(sitios(&t, malo), Err(Error::MalFormado), "{:?}", malo);
    }
    // Nombres que no hay.
    assert_eq!(sitios(&t, "Nada:x"), Err(Error::NoExiste));
    assert_eq!(sitios(&t, "Ana:x"), Err(Error::NoExiste));
    // Larguísimos: el nombre, el camino y el camino de una capa con su subcamino.
    let largo = "x".repeat(MAX_NOMBRE + 1);
    assert_eq!(sitios(&t, &format!("{}:a", largo)), Err(Error::MalFormado));
    assert_eq!(sitios(&t, &format!("SYS:{}", "a/".repeat(200))), Err(Error::MalFormado));
    let justo = "a".repeat(MAX_CAMINO);
    assert_eq!(sitios(&t, &format!("SYS:{}", justo)), Ok(v(&[(1, &justo)])));
    // En una capa con subcamino ("Home:C/" + 224) ya no cabe.
    assert_eq!(sitios(&t, &format!("C:{}", justo)), Err(Error::NoCabe));
}

#[test]
fn bucles_y_capas_que_faltan() {
    // Un bucle que llega en la tabla no vale entera; resolver lo para igual.
    assert_eq!(Tabla::desde(b"A=B:\nB=A:\n").err(), Some(Error::Bucle));
    assert_eq!(Tabla::desde(b"A=A:x\n").err(), Some(Error::Bucle));
    let mut t = de_persona();
    assert_eq!(t.asignar(Cambio::Poner, b"Uno=Dos:x"), Ok(()));
    assert_eq!(t.asignar(Cambio::Poner, b"Dos=Uno:y"), Err(Error::Bucle));
    assert!(t.buscar(b"Dos").is_none(), "el bucle no se queda");
    assert_eq!(t.asignar(Cambio::Anadir, b"Uno=Uno:"), Err(Error::Bucle));
    assert_eq!(t.buscar(b"Uno").unwrap().destinos, b"Dos:x");
    // Una cadena de 8 niveles es un bucle (como dos); de 7, no.
    let mut c = Tabla::vacia();
    c.poner_volumen(b"V", 1, b"").unwrap();
    c.asignar(Cambio::Poner, b"N0=V:").unwrap();
    for i in 1..MAX_NIVELES - 1 {
        c.asignar(Cambio::Poner, format!("N{}=N{}:", i, i - 1).as_bytes()).unwrap();
    }
    assert_eq!(sitios(&c, &format!("N{}:x", MAX_NIVELES - 2)), Ok(v(&[(1, "x")])));
    assert_eq!(c.asignar(Cambio::Poner, format!("N{}=N{}:", MAX_NIVELES - 1, MAX_NIVELES - 2).as_bytes()), Err(Error::Bucle));
    // Una capa que lleva a un volumen que no hay, no está: un proceso sin Home: ve C: solo en SYS:.
    let mut s = Tabla::vacia();
    s.poner_volumen(b"SYS", 1, b"").unwrap();
    de_sesion(&mut s, false).unwrap();
    assert_eq!(sitios(&s, "C:List"), Ok(v(&[(1, "C/List")])));
    // Y ninguna capa: no existe.
    assert_eq!(sitios(&s, "ENV:x"), Err(Error::NoExiste));
}

#[test]
fn asignar_no_tapa_ni_pone_raices() {
    let mut t = de_persona();
    let antes = t.texto().to_vec();
    // No se tapan volúmenes ni PROGDIR, ni se quitan.
    assert_eq!(t.asignar(Cambio::Poner, b"SYS=Home:Falso"), Err(Error::Protegido));
    assert_eq!(t.asignar(Cambio::Poner, b"sys=Home:Falso"), Err(Error::Protegido));
    assert_eq!(t.asignar(Cambio::Quitar, b"Home"), Err(Error::Protegido));
    assert_eq!(t.asignar(Cambio::Poner, b"PROGDIR=Home:"), Err(Error::Protegido));
    assert_eq!(t.asignar(Cambio::Poner, b"progdir=Home:"), Err(Error::Protegido));
    // Ni el nombre de un volumen que este proceso no tiene: lo tendría quizá lo que lance.
    let mut sin = Tabla::vacia();
    assert_eq!(sin.asignar(Cambio::Poner, b"Work=SYS:Libs"), Err(Error::Protegido));
    assert_eq!(sin.asignar(Cambio::Poner, b"ram=SYS:Libs"), Err(Error::Protegido));
    // Nunca raíces directas: las pone sesiones.
    assert_eq!(t.asignar(Cambio::Poner, b"X=@1"), Err(Error::MalFormado));
    assert_eq!(t.asignar(Cambio::Poner, b"X=@5/a"), Err(Error::MalFormado));
    assert_eq!(t.asignar(Cambio::Anadir, b"C=@1"), Err(Error::MalFormado));
    assert_eq!(t.asignar(Cambio::Poner, b"X=Home:a,@1"), Err(Error::MalFormado));
    // Mal escritos.
    for malo in [&b"X="[..], b"=Home:", b"X:=Home:", b"X=Home", b"X=Home:/a", b"X=Home:..", b"X=Home:a/", b"X=:a", b"X=Home:a,,SYS:b", b"X Y=Home:", b"X\n*SYS=@9"] {
        assert_eq!(t.asignar(Cambio::Poner, malo), Err(Error::MalFormado), "{:?}", String::from_utf8_lossy(malo));
    }
    assert_eq!(t.asignar(Cambio::Quitar, b"X=Home:"), Err(Error::MalFormado));
    assert_eq!(t.asignar(Cambio::Quitar, b"Nada"), Err(Error::NoExiste));
    let largo = format!("{}=Home:", "N".repeat(MAX_NOMBRE + 1));
    assert_eq!(t.asignar(Cambio::Poner, largo.as_bytes()), Err(Error::MalFormado));
    // Demasiadas capas.
    let muchas = format!("X={}", vec!["Home:a"; MAX_CAPAS + 1].join(","));
    assert_eq!(t.asignar(Cambio::Poner, muchas.as_bytes()), Err(Error::NoCabe));
    assert_eq!(t.texto(), &antes[..], "lo que falla no cambia nada");
    // Lo que sí: poner, añadir detrás y delante, quitar.
    t.asignar(Cambio::Poner, b"Trabajo=Home:Proyectos").unwrap();
    t.asignar(Cambio::Anadir, b"trabajo=Work:Comun").unwrap();
    t.asignar(Cambio::Delante, b"TRABAJO=RAM:T").unwrap();
    assert_eq!(sitios(&t, "Trabajo:a"), Ok(v(&[(3, "T/a"), (2, "Proyectos/a"), (4, "Comun/a")])));
    t.asignar(Cambio::Anadir, b"LIBS=Work:Libs").unwrap();
    assert_eq!(sitios(&t, "LIBS:"), Ok(v(&[(2, "Libs"), (1, "Libs"), (4, "Libs")])));
    t.asignar(Cambio::Quitar, b"Trabajo").unwrap();
    assert_eq!(sitios(&t, "Trabajo:a"), Err(Error::NoExiste));
}

#[test]
fn tabla_que_llega_mal() {
    for malo in [
        &b"SYS=@1\n"[..],     // una raíz directa sin ser volumen
        b"*SYS=Home:x\n",       // un volumen que no es una raíz
        b"*SYS=@1,@2\n",        // un volumen con dos
        b"*SYS=@16\n",          // una raíz que no hay
        b"*SYS=@01x\n",
        b"*SYS=@\n",
        b"*SYS=@1/\n",
        b"*SYS=@1/../x\n",
        b"*SYS=@1\n*sys=@2\n",  // repetido
        b"PROGDIR=SYS:\n",
        b"*S:YS=@1\n",
        b"C\n",
    ] {
        assert!(Tabla::desde(malo).is_err(), "{:?}", String::from_utf8_lossy(malo));
    }
    assert_eq!(Tabla::desde(&[b'a'; MAX_TABLA + 1]).err(), Some(Error::NoCabe));
    // Lo que no cabe en la tabla no se pone, y la tabla sigue igual.
    let mut t = de_persona();
    let mut i = 0;
    while t.asignar(Cambio::Poner, format!("Relleno{}=Home:{}", i, "z".repeat(40)).as_bytes()).is_ok() {
        i += 1;
    }
    assert!(i > 2 && t.texto().len() <= MAX_TABLA);
    assert_eq!(Tabla::desde(t.texto()).unwrap().texto(), t.texto());
}

#[test]
fn heredar_y_encerrado() {
    let mut padre = de_persona();
    padre.asignar(Cambio::Poner, b"Trabajo=Home:Proyectos").unwrap();
    padre.asignar(Cambio::Anadir, b"LIBS=Work:Libs").unwrap();
    // El hijo tiene otras raíces: SYS en la 1 y Home en la 5, sin RAM ni Work, y su PROGDIR.
    let mut propia = Tabla::vacia();
    propia.poner_volumen(b"SYS", 1, b"").unwrap();
    propia.poner_volumen(b"Home", 5, b"").unwrap();
    propia.poner_progdir(0).unwrap();
    let hijo = Tabla::heredar(&padre, &propia);
    assert_eq!(sitios(&hijo, "Trabajo:x"), Ok(v(&[(5, "Proyectos/x")])));
    assert_eq!(sitios(&hijo, "LIBS:"), Ok(v(&[(5, "Libs"), (1, "Libs")])), "Work: no está: su capa no cuenta");
    assert_eq!(sitios(&hijo, "ENV:"), Err(Error::NoExiste));
    // Los volúmenes del padre no pasan.
    assert!(hijo.buscar(b"Work").is_none() && hijo.buscar(b"RAM").is_none());
    // Un padre con un volumen o un PROGDIR "suyo" no se los pasa.
    let mut otro = Tabla::vacia();
    otro.poner_volumen(b"Home", 9, b"").unwrap();
    otro.poner_progdir(7).unwrap();
    let h = Tabla::heredar(&otro, &propia);
    assert_eq!(sitios(&h, "Home:"), Ok(v(&[(5, "")])));
    assert_eq!(sitios(&h, "PROGDIR:"), Ok(v(&[(0, "")])));
    // Encerrado: sin la capa del usuario en C:, LIBS:, FONTS: y CLASSES:; las demás, con ella.
    let mut e = Tabla::vacia();
    e.poner_volumen(b"SYS", 1, b"").unwrap();
    e.poner_volumen(b"Home", 2, b"").unwrap();
    de_sesion(&mut e, true).unwrap();
    assert_eq!(sitios(&e, "LIBS:x"), Ok(v(&[(1, "Libs/x")])));
    assert_eq!(sitios(&e, "C:x"), Ok(v(&[(1, "C/x")])));
    assert_eq!(sitios(&e, "FONTS:x"), Ok(v(&[(1, "Fonts/x")])));
    assert_eq!(sitios(&e, "CLASSES:x"), Ok(v(&[(1, "Classes/x")])));
    assert_eq!(sitios(&e, "S:x"), Ok(v(&[(2, "S/x"), (1, "S/x")])));
}

// ---------------------------------------------------------------------------------------------------------------
// Con un servicio de archivos falso.

#[derive(Clone)]
struct Nodo {
    nombre: String,
    cajon: bool,
    escribible: bool,
    hijos: Vec<usize>,
    vivo: bool,
}

struct Falso {
    nodos: Vec<Nodo>,
    /// Hueco de raíz: el nodo y si se escribe por ella.
    raices: BTreeMap<u32, (usize, bool)>,
    asas: BTreeMap<u32, (usize, bool)>,
    siguiente: u32,
    ventana: Vec<u8>,
    peticiones: Vec<(u32, u32, String)>,
}

impl Falso {
    fn nuevo() -> Self {
        let raiz = Nodo { nombre: String::new(), cajon: true, escribible: false, hijos: vec![], vivo: true };
        Falso { nodos: vec![raiz], raices: BTreeMap::new(), asas: BTreeMap::new(), siguiente: 16, ventana: vec![0; 4096], peticiones: vec![] }
    }

    fn anadir(&mut self, padre: usize, nombre: &str, cajon: bool, escribible: bool) -> usize {
        self.nodos.push(Nodo { nombre: nombre.into(), cajon, escribible, hijos: vec![], vivo: true });
        let i = self.nodos.len() - 1;
        self.nodos[padre].hijos.push(i);
        i
    }

    /// Crea los cajones de `camino` desde `desde` (escribibles o no) y da el último.
    fn cajones(&mut self, desde: usize, camino: &str, escribible: bool) -> usize {
        let mut n = desde;
        for p in camino.split('/').filter(|p| !p.is_empty()) {
            n = match self.hijo(n, p) {
                Some(h) => h,
                None => self.anadir(n, p, true, escribible),
            };
        }
        n
    }

    fn hijo(&self, n: usize, nombre: &str) -> Option<usize> {
        self.nodos[n].hijos.iter().copied().find(|&h| self.nodos[h].vivo && self.nodos[h].nombre.eq_ignore_ascii_case(nombre))
    }

    fn asa(&self, asa: u32) -> Option<(usize, bool)> {
        if asa < 16 { self.raices.get(&asa).copied() } else { self.asas.get(&asa).copied() }
    }

    /// Un disco con SYS (raíz 1, solo leer): C/List, Libs/a.library y Libs/b.library, Prefs/Env-Archive/Sys/x.prefs,
    /// S/Startup-Sequence; Home (raíz 2, leer y escribir): Libs/a.library (la suya), Prefs/; RAM (raíz 3): T/.
    fn disco() -> Self {
        let mut f = Falso::nuevo();
        let sys = f.anadir(0, "SYS", true, false);
        let c = f.cajones(sys, "C", false);
        f.anadir(c, "List", false, false);
        let libs = f.cajones(sys, "Libs", false);
        f.anadir(libs, "a.library", false, false);
        f.anadir(libs, "b.library", false, false);
        let env = f.cajones(sys, "Prefs/Env-Archive/Sys", false);
        f.anadir(env, "x.prefs", false, false);
        let s = f.cajones(sys, "S", false);
        f.anadir(s, "Startup-Sequence", false, false);
        let home = f.anadir(0, "Ana", true, true);
        let hl = f.cajones(home, "Libs", true);
        f.anadir(hl, "a.library", false, true);
        f.cajones(home, "Prefs", true);
        let ram = f.anadir(0, "RAM", true, true);
        f.cajones(ram, "T", true);
        f.raices.insert(1, (sys, false));
        f.raices.insert(2, (home, true));
        f.raices.insert(3, (ram, true));
        f
    }

    fn existe(&self, raiz: u32, camino: &str) -> bool {
        let Some((mut n, _)) = self.raices.get(&raiz).copied() else { return false };
        for p in camino.split('/').filter(|p| !p.is_empty()) {
            match self.hijo(n, p) {
                Some(h) => n = h,
                None => return false,
            }
        }
        true
    }
}

impl Fs for Falso {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> Resp {
        let nombre = std::str::from_utf8(nombre).unwrap().to_string();
        self.peticiones.push((orden, asa, nombre.clone()));
        let mal = |e| Resp { e, ..Default::default() };
        let Some((n, w)) = self.asa(asa) else { return mal(a::ASA_MALA) };
        match orden {
            a::BUSCAR => {
                let mut m = n;
                for p in nombre.split('/') {
                    if p.is_empty() && nombre.is_empty() {
                        break;
                    }
                    if p.is_empty() || p == ".." || p.contains(':') {
                        return mal(a::MAL_FORMADO);
                    }
                    if !self.nodos[m].cajon {
                        return mal(a::NO_ES_CAJON);
                    }
                    match self.hijo(m, p) {
                        Some(h) => m = h,
                        None => return mal(a::NO_EXISTE),
                    }
                }
                let tipo = if self.nodos[m].cajon { a::CAJON } else { a::FICHERO } as u64;
                if x == 1 {
                    return Resp { c: tipo << 16, ..Default::default() };
                }
                let nueva = self.siguiente;
                self.siguiente += 1;
                self.asas.insert(nueva, (m, w));
                Resp { asa: nueva, c: tipo << 16, ..Default::default() }
            }
            a::CREAR | a::GUARDAR => {
                if !self.nodos[n].cajon {
                    return mal(a::NO_ES_CAJON);
                }
                if self.hijo(n, &nombre).is_some() {
                    return mal(a::YA_EXISTE);
                }
                if !w || !self.nodos[n].escribible {
                    return mal(a::PROTEGIDO);
                }
                let h = self.anadir(n, &nombre, orden == a::CREAR && x == a::CAJON as u64, true);
                if orden == a::GUARDAR {
                    return Resp { a: y, b: y, ..Default::default() };
                }
                let nueva = self.siguiente;
                self.siguiente += 1;
                self.asas.insert(nueva, (h, w));
                Resp { asa: nueva, ..Default::default() }
            }
            a::LISTAR => {
                if !self.nodos[n].cajon {
                    return mal(a::NO_ES_CAJON);
                }
                // Dos por página, para pasar por varias.
                let hijos: Vec<usize> = self.nodos[n].hijos.iter().copied().filter(|&h| self.nodos[h].vivo).collect();
                let mut o = 0;
                let mut cuantas = 0;
                let desde = x as usize;
                for &h in hijos.iter().skip(desde).take(2) {
                    let nom = self.nodos[h].nombre.clone();
                    let e = &mut self.ventana[o..];
                    e[..24].fill(0);
                    e[18] = if self.nodos[h].cajon { a::CAJON } else { a::FICHERO };
                    e[19] = nom.len() as u8;
                    e[24..24 + nom.len()].copy_from_slice(nom.as_bytes());
                    o += (24 + nom.len()).next_multiple_of(8);
                    cuantas += 1;
                }
                let siguiente = if desde + cuantas < hijos.len() { (desde + cuantas) as u64 } else { u64::MAX };
                Resp { a: cuantas as u64, b: siguiente, c: o as u64, ..Default::default() }
            }
            a::CERRAR => {
                if asa >= 16 {
                    self.asas.remove(&asa);
                }
                Resp::default()
            }
            _ => mal(a::MAL_FORMADO),
        }
    }

    fn ventana(&self) -> &[u8] {
        &self.ventana
    }
}

fn nombres_de(f: &mut Falso, t: &Tabla, c: &str) -> Result<Vec<(usize, String)>, i32> {
    let mut v = vec![];
    listar(f, t, c.as_bytes(), |capa, e| {
        v.push((capa, String::from_utf8(e.nombre.to_vec()).unwrap()));
        true
    })?;
    Ok(v)
}

#[test]
fn abrir_en_orden() {
    let mut f = Falso::disco();
    let t = de_persona();
    // La del usuario tapa a la del sistema; lo que solo está en el sistema, de ahí.
    assert_eq!(abrir(&mut f, &t, b"LIBS:a.library").map(|r| r.1), Ok(0));
    assert_eq!(abrir(&mut f, &t, b"LIBS:b.library").map(|r| r.1), Ok(1));
    assert_eq!(mirar(&mut f, &t, b"C:List").map(|r| r.1), Ok(1));
    assert_eq!(abrir(&mut f, &t, b"LIBS:c.library").err(), Some(a::NO_EXISTE));
    // Work: no es una raíz de este proceso: asa mala, que para quien busca es "no existe".
    assert_eq!(abrir(&mut f, &t, b"Work:x").err(), Some(a::NO_EXISTE));
    // ENVARC:: la capa del usuario no tiene Env-Archive; la del sistema, sí.
    assert_eq!(mirar(&mut f, &t, b"ENVARC:Sys/x.prefs").map(|r| r.1), Ok(1));
    // Lo mal escrito no llega al servicio.
    let antes = f.peticiones.len();
    for malo in [&b"LIBS:../x"[..], b"LIBS:/x", b"LIBS:a:b", b"LIBS::", b"A:B:C", b"sin dos puntos"] {
        assert_eq!(abrir(&mut f, &t, malo).err(), Some(a::MAL_FORMADO), "{:?}", String::from_utf8_lossy(malo));
    }
    assert_eq!(f.peticiones.len(), antes);
}

#[test]
fn una_capa_que_desaparece() {
    let mut f = Falso::disco();
    let t = de_persona();
    assert_eq!(abrir(&mut f, &t, b"LIBS:a.library").map(|r| r.1), Ok(0));
    // La raíz de Home se va (una instantánea vuelta atrás, un volumen cerrado): LIBS: sigue por SYS:.
    f.raices.remove(&2);
    assert_eq!(abrir(&mut f, &t, b"LIBS:a.library").map(|r| r.1), Ok(1));
    assert_eq!(nombres_de(&mut f, &t, "LIBS:"), Ok(vec![(1, "a.library".into()), (1, "b.library".into())]));
    // Y crear ya no puede en ninguna: Home no está y SYS: no deja.
    assert_eq!(crear(&mut f, &t, b"LIBS:nueva.library", a::FICHERO).err(), Some(a::PROTEGIDO));
}

#[test]
fn crear_en_la_primera_capa_que_deja() {
    let mut f = Falso::disco();
    let t = de_persona();
    // LIBS: tiene Home:Libs (se puede): ahí.
    assert_eq!(crear(&mut f, &t, b"LIBS:nueva.library", a::FICHERO).map(|r| r.1), Ok(0));
    assert!(f.existe(2, "Libs/nueva.library") && !f.existe(1, "Libs/nueva.library"));
    // Ya existe en esa capa.
    assert_eq!(crear(&mut f, &t, b"LIBS:nueva.library", a::FICHERO).err(), Some(a::YA_EXISTE));
    // Está solo en SYS: (b.library): se crea la del usuario, que la tapa.
    assert_eq!(crear(&mut f, &t, b"LIBS:b.library", a::FICHERO).map(|r| r.1), Ok(0));
    // C: no tiene Home:C: pasa a SYS:C, que no deja. Protegido, y nada en SYS:.
    assert_eq!(crear(&mut f, &t, b"C:Nuevo", a::FICHERO).err(), Some(a::PROTEGIDO));
    assert!(!f.existe(1, "C/Nuevo"));
    // Con Home:C hecho, va ahí.
    assert_eq!(crear(&mut f, &t, b"Home:C", a::CAJON).map(|r| r.1), Ok(0));
    assert_eq!(guardar(&mut f, &t, b"C:Nuevo", 10).map(|r| r.1), Ok(0));
    assert!(f.existe(2, "C/Nuevo"));
    // Sin cajón padre en ninguna capa: no existe.
    assert_eq!(crear(&mut f, &t, b"ENVARC:Otro/x", a::FICHERO).err(), Some(a::NO_EXISTE));
    // El volumen mismo no se crea.
    assert_eq!(crear(&mut f, &t, b"Home:", a::CAJON).err(), Some(a::YA_EXISTE));
    // T: es RAM:T.
    assert_eq!(crear(&mut f, &t, b"T:temporal", a::FICHERO).map(|r| r.1), Ok(0));
    assert!(f.existe(3, "T/temporal"));
    // Ninguna asa se queda abierta salvo las cuatro que devolvió crear (guardar no deja ninguna).
    assert_eq!(f.asas.len(), 4);
}

#[test]
fn listar_une_las_capas() {
    let mut f = Falso::disco();
    let t = de_persona();
    // a.library está en las dos: sale una vez, la del usuario.
    assert_eq!(nombres_de(&mut f, &t, "LIBS:"), Ok(vec![(0, "a.library".into()), (1, "b.library".into())]));
    assert_eq!(nombres_de(&mut f, &t, "libs:").unwrap().len(), 2);
    // Sin Home:C, C: es la de SYS:.
    assert_eq!(nombres_de(&mut f, &t, "C:"), Ok(vec![(1, "List".into())]));
    // Algo que no es un cajón en ninguna capa, o que no existe: no se lista.
    assert_eq!(nombres_de(&mut f, &t, "Nada:"), Err(a::NO_EXISTE));
    assert_eq!(nombres_de(&mut f, &t, "LIBS:no-hay"), Err(a::NO_EXISTE));
    // Parar a mitad.
    let mut n = 0;
    assert_eq!(listar(&mut f, &t, b"LIBS:", |_, _| { n += 1; false }), Ok(1));
    assert_eq!(n, 1);
    // Todas las asas cerradas.
    assert!(f.asas.is_empty(), "{:?}", f.asas);
}

#[test]
fn encerrado_no_llega_por_un_nombre() {
    // Un programa encerrado: solo su cajón (raíz 0) y SYS: para leer (raíz 1). Una tabla escrita a mano que dice que
    // Home: es la raíz 2 no le da nada: el servicio no le conoce esa raíz.
    let mut f = Falso::disco();
    f.raices.remove(&2);
    f.raices.remove(&3);
    let mut propia = Tabla::vacia();
    propia.poner_volumen(b"SYS", 1, b"").unwrap();
    propia.poner_progdir(0).unwrap();
    let mut t = Tabla::heredar(&de_persona(), &propia);
    de_sesion(&mut t, true).unwrap();
    assert_eq!(abrir(&mut f, &t, b"Home:Libs/a.library").err(), Some(a::NO_EXISTE));
    assert_eq!(abrir(&mut f, &t, b"LIBS:a.library").map(|r| r.1), Ok(0), "solo la de SYS: (su capa 0)");
    let falsa = Tabla::desde(b"*SYS=@1\n*Home=@2\nLIBS=Home:Libs\n").unwrap();
    assert_eq!(abrir(&mut f, &falsa, b"LIBS:a.library").err(), Some(a::NO_EXISTE));
    assert_eq!(crear(&mut f, &falsa, b"Home:x", a::FICHERO).err(), Some(a::NO_EXISTE));
    // Y por ASIGNAR no se pone una raíz.
    assert_eq!(t.asignar(Cambio::Poner, b"Casa=@2"), Err(Error::MalFormado));
}
