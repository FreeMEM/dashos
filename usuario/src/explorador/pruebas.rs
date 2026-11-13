//! Las pruebas del explorador en el PC, sin DashOS: `rustc --edition 2024 --test usuario/src/explorador/pruebas.rs`.
//! Lo del servicio, contra un servicio de archivos falso en memoria (`Falso`) que hace lo que dice
//! `docs/referencia/servicios.md` en lo que usa el explorador: asas, raíces con tope, dueños y permisos, no sobrescribir,
//! no meter un cajón en sí mismo y un disco que se llena.

extern crate alloc;

#[path = "../protocolo/archivos.rs"]
#[allow(dead_code)]
mod archivos;
#[path = "logica.rs"]
#[allow(dead_code)]
mod logica;
#[path = "ops.rs"]
#[allow(dead_code)]
mod ops;

use archivos as a;
use logica::*;
use ops::*;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------------------------------------------
// El servicio falso.

#[derive(Clone, Debug)]
struct Nodo {
    nombre: String,
    cajon: bool,
    datos: Vec<u8>,
    hijos: Vec<usize>,
    padre: usize,
    dueno: u32,
    modo: u16,
    bits: u16,
    vivo: bool,
}

struct Falso {
    nodos: Vec<Nodo>,
    asas: BTreeMap<u32, (usize, bool)>,
    siguiente: u32,
    ventana: Vec<u8>,
    yo: u32,
    /// Lo que cabe en el disco (bytes de datos).
    cabe: usize,
    peticiones: usize,
}

const RAIZ_SISTEMA: u32 = 1;
const RAIZ_HOME: u32 = 2;

impl Falso {
    /// Un disco con: C (Home de Ana) con nota.txt, Fotos/{a.png, b.png}, Privado/carta.txt; DeCarla (0700, de Carla) con
    /// secreto.txt; Programas/hola.
    fn nuevo() -> Self {
        let raiz = Nodo { nombre: String::new(), cajon: true, datos: vec![], hijos: vec![], padre: 0, dueno: 1000, modo: 0o755, bits: 0, vivo: true };
        let mut f = Falso { nodos: vec![raiz], asas: BTreeMap::new(), siguiente: 16, ventana: vec![0; 4096], yo: 1000, cabe: 1 << 20, peticiones: 0 };
        let c = f.anadir(0, "C", true, b"", 1000, 0o755);
        f.anadir(c, "nota.txt", false, b"hola, soy una nota", 1000, 0o644);
        let fotos = f.anadir(c, "Fotos", true, b"", 1000, 0o755);
        f.anadir(fotos, "a.png", false, &[7u8; 10_000], 1000, 0o644);
        f.anadir(fotos, "b.png", false, &[9u8; 5000], 1000, 0o600);
        let p = f.anadir(c, "Privado", true, b"", 1000, 0o700);
        f.anadir(p, "carta.txt", false, b"querida Ana", 1000, 0o600);
        let carla = f.anadir(0, "DeCarla", true, b"", 1001, 0o700);
        f.anadir(carla, "secreto.txt", false, b"de Carla", 1001, 0o600);
        let pr = f.anadir(0, "Programas", true, b"", 1, 0o755);
        f.anadir(pr, "hola", false, b"\x7fELF", 1, 0o755);
        // Las raíces: Sistema (el disco, solo leer) y Home (C, leer y escribir).
        f.asas.insert(RAIZ_SISTEMA, (0, false));
        f.asas.insert(RAIZ_HOME, (c, true));
        f
    }

    fn anadir(&mut self, padre: usize, nombre: &str, cajon: bool, datos: &[u8], dueno: u32, modo: u16) -> usize {
        self.nodos.push(Nodo { nombre: nombre.into(), cajon, datos: datos.to_vec(), hijos: vec![], padre, dueno, modo, bits: 0, vivo: true });
        let i = self.nodos.len() - 1;
        self.nodos[padre].hijos.push(i);
        i
    }

    fn usado(&self) -> usize {
        self.nodos.iter().filter(|n| n.vivo).map(|n| n.datos.len()).sum()
    }

    fn puede(&self, n: usize, que: u16) -> bool {
        let x = &self.nodos[n];
        let m = if x.dueno == self.yo { x.modo >> 6 } else { x.modo };
        m & que == que
    }

    fn hijo(&self, dir: usize, nombre: &str) -> Option<usize> {
        self.nodos[dir].hijos.iter().copied().find(|&h| self.nodos[h].vivo && self.nodos[h].nombre.to_lowercase() == nombre.to_lowercase())
    }

    /// Lo que hay en un camino desde la raíz ("C/Fotos/a.png"), para mirar el resultado.
    fn en(&self, camino: &str) -> Option<&Nodo> {
        let mut n = 0;
        for p in camino.split('/').filter(|p| !p.is_empty()) {
            n = self.hijo(n, p)?;
        }
        Some(&self.nodos[n])
    }

    fn nombres(&self, camino: &str) -> Vec<String> {
        let n = self.en(camino).unwrap();
        let mut v: Vec<String> = n.hijos.iter().filter(|&&h| self.nodos[h].vivo).map(|&h| self.nodos[h].nombre.clone()).collect();
        v.sort();
        v
    }

    fn dar(&mut self, n: usize, w: bool) -> Resp {
        if self.asas.len() >= 256 {
            return Resp { e: a::SIN_SITIO, ..Default::default() };
        }
        let asa = self.siguiente;
        self.siguiente += 1;
        self.asas.insert(asa, (n, w));
        self.datos(n, asa)
    }

    fn datos(&self, n: usize, asa: u32) -> Resp {
        let x = &self.nodos[n];
        let tipo = if x.cajon { a::CAJON } else { a::FICHERO } as u64;
        Resp { e: a::BIEN, asa, a: x.datos.len() as u64, b: 0, c: x.modo as u64 | tipo << 16 | (x.bits as u64) << 32 }
    }

    fn mal(e: i32) -> Resp {
        Resp { e, ..Default::default() }
    }
}

impl Fs for Falso {
    fn pedir(&mut self, orden: u32, asa: u32, x: u64, y: u64, nombre: &[u8]) -> Resp {
        self.peticiones += 1;
        let Some(&(n, w)) = self.asas.get(&asa) else { return Falso::mal(a::ASA_MALA) };
        let nombre = std::str::from_utf8(nombre).unwrap_or("\u{fffd}");
        let un = |t: &str| !t.is_empty() && !t.contains('/') && !t.contains('\0') && t != "." && t != "..";
        match orden {
            a::BUSCAR => {
                if !self.puede(n, 1) {
                    return Falso::mal(a::PROTEGIDO);
                }
                if !un(nombre) {
                    return Falso::mal(a::NO_EXISTE);
                }
                match self.hijo(n, nombre) {
                    Some(h) => self.dar(h, w),
                    None => Falso::mal(a::NO_EXISTE),
                }
            }
            a::LISTAR => {
                if !self.nodos[n].cajon {
                    return Falso::mal(a::NO_ES_CAJON);
                }
                if !self.puede(n, 4) {
                    return Falso::mal(a::PROTEGIDO);
                }
                let hijos: Vec<usize> = self.nodos[n].hijos.iter().copied().filter(|&h| self.nodos[h].vivo).collect();
                let desde = x as usize;
                let mut off = 0;
                let mut cuantos = 0;
                let mut k = desde;
                while k < hijos.len() {
                    let h = &self.nodos[hijos[k]];
                    let t = 24 + h.nombre.len();
                    if off + t > self.ventana.len() {
                        break;
                    }
                    let v = &mut self.ventana[off..off + t];
                    v[0..8].copy_from_slice(&(h.datos.len() as u64).to_le_bytes());
                    v[8..16].copy_from_slice(&0u64.to_le_bytes());
                    v[16..18].copy_from_slice(&h.modo.to_le_bytes());
                    v[18] = if h.cajon { a::CAJON } else { a::FICHERO };
                    v[19] = h.nombre.len() as u8;
                    v[20..22].copy_from_slice(&h.bits.to_le_bytes());
                    v[22] = 0;
                    v[23] = 0;
                    v[24..].copy_from_slice(h.nombre.as_bytes());
                    off += t.next_multiple_of(8);
                    cuantos += 1;
                    k += 1;
                }
                let sig = if k >= hijos.len() { u64::MAX } else { k as u64 };
                Resp { e: a::BIEN, asa, a: cuantos, b: sig, c: off as u64 }
            }
            a::CERRAR => {
                if asa >= a::RAICES {
                    self.asas.remove(&asa);
                }
                Resp::default()
            }
            a::CREAR | a::GUARDAR => {
                if !w {
                    return Falso::mal(a::PROTEGIDO);
                }
                if !self.puede(n, 3) {
                    return Falso::mal(a::PROTEGIDO);
                }
                if !un(nombre) {
                    return Falso::mal(a::MAL_FORMADO);
                }
                if self.hijo(n, nombre).is_some() {
                    return Falso::mal(a::YA_EXISTE);
                }
                let cajon = orden == a::CREAR && x == a::CAJON as u64;
                let datos = if orden == a::GUARDAR { self.ventana[..y as usize].to_vec() } else { vec![] };
                if self.usado() + datos.len() > self.cabe {
                    return Falso::mal(a::LLENO);
                }
                let yo = self.yo;
                let h = self.anadir(n, nombre, cajon, &datos, yo, if cajon { 0o755 } else { 0o644 });
                if orden == a::GUARDAR {
                    return Resp { e: a::BIEN, a: y, b: y, ..Default::default() };
                }
                self.dar(h, w)
            }
            a::LEER | a::LEER_NOMBRE => {
                let n = if orden == a::LEER_NOMBRE {
                    match self.hijo(n, nombre) {
                        Some(h) => h,
                        None => return Falso::mal(a::NO_EXISTE),
                    }
                } else {
                    n
                };
                if !self.puede(n, 4) {
                    return Falso::mal(a::PROTEGIDO);
                }
                let d = &self.nodos[n].datos;
                let desde = (x as usize).min(d.len());
                let cuanto = (y as usize).min(d.len() - desde).min(self.ventana.len());
                let trozo = d[desde..desde + cuanto].to_vec();
                self.ventana[..cuanto].copy_from_slice(&trozo);
                Resp { e: a::BIEN, a: cuanto as u64, b: self.nodos[n].datos.len() as u64, ..Default::default() }
            }
            a::ESCRIBIR => {
                if !w || !self.puede(n, 2) {
                    return Falso::mal(a::PROTEGIDO);
                }
                let (desde, cuanto) = (x as usize, y as usize);
                let crece = (desde + cuanto).saturating_sub(self.nodos[n].datos.len());
                if self.usado() + crece > self.cabe {
                    return Falso::mal(a::LLENO);
                }
                let trozo = self.ventana[..cuanto].to_vec();
                let d = &mut self.nodos[n].datos;
                if d.len() < desde + cuanto {
                    d.resize(desde + cuanto, 0);
                }
                d[desde..desde + cuanto].copy_from_slice(&trozo);
                Resp { e: a::BIEN, a: cuanto as u64, b: d.len() as u64, ..Default::default() }
            }
            a::BORRAR => {
                if !w || !self.puede(n, 3) {
                    return Falso::mal(a::PROTEGIDO);
                }
                let Some(h) = self.hijo(n, nombre) else { return Falso::mal(a::NO_EXISTE) };
                if self.nodos[h].hijos.iter().any(|&k| self.nodos[k].vivo) {
                    return Falso::mal(a::NO_VACIO);
                }
                self.nodos[h].vivo = false;
                Resp::default()
            }
            a::RENOMBRAR => {
                // El cajón de destino va en b (bytes 16 a 23), como en el servicio.
                let Some(&(d, wd)) = self.asas.get(&(y as u32)).filter(|_| y <= u32::MAX as u64) else { return Falso::mal(a::ASA_MALA) };
                if !w || !wd || !self.puede(n, 3) || !self.puede(d, 3) {
                    return Falso::mal(a::PROTEGIDO);
                }
                let mut partes = nombre.splitn(2, '\0');
                let (Some(v), Some(nu)) = (partes.next(), partes.next()) else { return Falso::mal(a::MAL_FORMADO) };
                if !un(v) || !un(nu) {
                    return Falso::mal(a::MAL_FORMADO);
                }
                let Some(h) = self.hijo(n, v) else { return Falso::mal(a::NO_EXISTE) };
                if self.hijo(d, nu).is_some() {
                    return Falso::mal(a::YA_EXISTE);
                }
                // No meter un cajón en sí mismo.
                let mut k = d;
                loop {
                    if k == h {
                        return Falso::mal(a::MAL_FORMADO);
                    }
                    if k == 0 {
                        break;
                    }
                    k = self.nodos[k].padre;
                }
                self.nodos[n].hijos.retain(|&x| x != h);
                self.nodos[d].hijos.push(h);
                self.nodos[h].padre = d;
                self.nodos[h].nombre = nu.into();
                Resp::default()
            }
            a::PROTEGER => {
                if !w || self.nodos[n].dueno != self.yo {
                    return Falso::mal(a::PROTEGIDO);
                }
                self.nodos[n].bits = x as u16;
                self.nodos[n].modo = y as u16;
                Resp::default()
            }
            _ => Falso::mal(a::MAL_FORMADO),
        }
    }

    fn ventana(&self) -> &[u8] {
        &self.ventana
    }

    fn ventana_mut(&mut self) -> &mut [u8] {
        &mut self.ventana
    }

    fn ahora(&self) -> u64 {
        1_763_000_000
    }
}

fn home(n: &[&str]) -> Lugar {
    Lugar { raiz: RAIZ_HOME, nombres: n.iter().map(|x| String::from(*x)).collect() }
}

fn el(desde: Lugar, nombre: &str, cajon: bool) -> Elemento {
    Elemento { desde, nombre: nombre.into(), cajon }
}

fn hasta_el_final(t: &mut Trabajo, f: &mut Falso) -> usize {
    let mut vueltas = 0;
    while !t.paso(f, 3) {
        vueltas += 1;
        assert!(vueltas < 100_000, "el trabajo no acaba");
    }
    vueltas
}

/// La papelera de Home, creada.
fn con_papelera(f: &mut Falso) -> Lugar {
    let c = f.asas[&RAIZ_HOME].0;
    let p = f.anadir(c, PAPELERA, true, b"", 1000, 0o700);
    f.anadir(p, PAPELERA_ARCHIVOS, true, b"", 1000, 0o700);
    f.anadir(p, PAPELERA_INFO, true, b"", 1000, 0o700);
    home(&[PAPELERA])
}

// ---------------------------------------------------------------------------------------------------------------
// Lógica.

#[test]
fn nombres_raros_no_valen() {
    for n in ["", ".", "..", "a/b", "x:y", "uno|dos", "tab\tdos", "fin\n", &"x".repeat(201)] {
        assert!(!nombre_valido(n), "{:?}", n);
    }
    for n in ["nota.txt", ".oculto", "ñandú", "con espacios", "a.b.c"] {
        assert!(nombre_valido(n), "{:?}", n);
    }
}

#[test]
fn orden_natural_y_carpetas_primero() {
    let e = |n: &str, cajon: bool, tam: u64| Entrada { nombre: n.into(), cajon, tam, fecha: 0, modo: 0o644, bits: 0 };
    let v = vec![e("foto 10.png", false, 5), e("Zeta", true, 0), e("foto 2.png", false, 50), e("alfa", true, 0), e("Beta.txt", false, 1)];
    let mut idx: Vec<usize> = (0..v.len()).collect();
    ordenar(&v, &mut idx, Orden::Nombre, true);
    let nombres: Vec<&str> = idx.iter().map(|&i| v[i].nombre.as_str()).collect();
    assert_eq!(nombres, ["alfa", "Zeta", "Beta.txt", "foto 2.png", "foto 10.png"]);
    ordenar(&v, &mut idx, Orden::Tamano, false);
    let nombres: Vec<&str> = idx.iter().map(|&i| v[i].nombre.as_str()).collect();
    assert_eq!(nombres, ["Zeta", "alfa", "foto 2.png", "foto 10.png", "Beta.txt"]);
}

#[test]
fn seleccion_con_ctrl_mayus_y_rectangulo() {
    let mut s = Seleccion::nueva(6);
    s.clic(1, false, false);
    s.clic(4, false, true);
    assert_eq!(s.elegidos(), [1, 2, 3, 4]);
    s.clic(2, true, false);
    assert_eq!(s.elegidos(), [1, 3, 4]);
    s.clic(5, false, false);
    assert_eq!(s.elegidos(), [5]);
    let antes = s.marcados.clone();
    s.rectangulo(&antes, &[0, 1], true);
    assert_eq!(s.elegidos(), [0, 1, 5]);
    s.rectangulo(&antes, &[0, 1], false);
    assert_eq!(s.elegidos(), [0, 1]);
    s.todo();
    assert_eq!(s.cuantos(), 6);
    s.mover(3, false, false);
    s.mover(5, false, true);
    assert_eq!(s.elegidos(), [3, 4, 5]);
    // Fuera de rango, nada.
    s.clic(99, false, false);
    assert_eq!(s.elegidos(), [3, 4, 5]);
}

#[test]
fn cursor_en_la_rejilla() {
    // 10 en 4 columnas: 0-3, 4-7, 8-9.
    assert_eq!(mover_cursor(None, Paso::Derecha, 10, 4, 2), Some(0));
    assert_eq!(mover_cursor(Some(5), Paso::Abajo, 10, 4, 2), Some(9));
    assert_eq!(mover_cursor(Some(9), Paso::Abajo, 10, 4, 2), Some(9));
    assert_eq!(mover_cursor(Some(2), Paso::Arriba, 10, 4, 2), Some(2));
    assert_eq!(mover_cursor(Some(6), Paso::Arriba, 10, 4, 2), Some(2));
    assert_eq!(mover_cursor(Some(0), Paso::Izquierda, 10, 4, 2), Some(0));
    assert_eq!(mover_cursor(Some(1), Paso::PaginaAbajo, 10, 4, 2), Some(9));
    assert_eq!(mover_cursor(Some(3), Paso::Fin, 0, 4, 2), None);
}

#[test]
fn buscar_escribiendo() {
    let n = ["Árbol", "casa", "Cosa", "zeta"];
    assert_eq!(buscar_al_escribir(&n, "co", 0), Some(2));
    assert_eq!(buscar_al_escribir(&n, "ar", 0), Some(0));
    assert_eq!(buscar_al_escribir(&n, "c", 2), Some(2));
    assert_eq!(buscar_al_escribir(&n, "x", 0), None);
    assert!(casa("Foto de Ávila.png", "avila"));
    assert!(!casa("nota", "z"));
}

#[test]
fn caminos_escritos_no_salen_de_la_raiz() {
    let raices = [("Home", 2), ("Sistema", 1)];
    let aqui = home(&["Fotos"]);
    assert_eq!(interpretar_camino("Home/Privado", &raices, &aqui), Ok(home(&["Privado"])));
    assert_eq!(interpretar_camino("home:Privado/x/", &raices, &aqui), Ok(home(&["Privado", "x"])));
    assert_eq!(interpretar_camino("Sistema", &raices, &aqui), Ok(Lugar::raiz(1)));
    assert_eq!(interpretar_camino("viaje", &raices, &aqui), Ok(home(&["Fotos", "viaje"])));
    for malo in ["Home/../Sistema", "..", "Home/./x", "Home//x", "Nada:x", "Home/a\tb", "", "Home/x|y"] {
        assert!(interpretar_camino(malo, &raices, &aqui).is_err(), "{:?}", malo);
    }
    let hondo = (0..40).map(|_| "a").collect::<Vec<_>>().join("/");
    assert!(interpretar_camino(&hondo, &raices, &aqui).is_err());
}

#[test]
fn nombres_de_copias() {
    let hay = ["nota.txt", "nota (copia).txt", "Fotos"];
    let existe = |n: &str| hay.contains(&n);
    assert_eq!(nombre_libre("nota.txt", false, existe, "copia"), "nota (copia 2).txt");
    assert_eq!(nombre_libre("Fotos", true, existe, "copia"), "Fotos (copia)");
    assert_eq!(nombre_libre("otra", false, existe, "copia"), "otra");
    assert_eq!(nombre_libre("nota.txt", false, existe, ""), "nota (2).txt");
    let largo = "x".repeat(199) + ".txt";
    let n = nombre_libre(&largo, false, |n| n == largo, "copia");
    assert!(n.len() <= 200 && n.ends_with(" (copia).txt"));
}

#[test]
fn fichas_de_la_papelera() {
    let f = Ficha { origen: home(&["Fotos", "Viaje"]), nombre: "a.png".into(), fecha: 123 };
    assert_eq!(Ficha::leer(f.texto().as_bytes(), &[1, 2]), Some(f.clone()));
    // Retocada a mala idea: raíz que no vale, "..", control, líneas raras.
    for mala in [
        "dashos-papelera 1\nraiz 7\nnombre a\n",
        "dashos-papelera 1\nraiz 2\nnombre a\ncajon ..\n",
        "dashos-papelera 1\nraiz 2\nnombre a/b\n",
        "dashos-papelera 1\nraiz 2\nnombre a\nsalto 3\n",
        "otra cosa\nraiz 2\nnombre a\n",
        "dashos-papelera 1\nraiz 2\n",
    ] {
        assert_eq!(Ficha::leer(mala.as_bytes(), &[1, 2]), None, "{:?}", mala);
    }
}

#[test]
fn textos() {
    assert_eq!(tamano(1), "1 byte");
    assert_eq!(tamano(18), "18 bytes");
    assert_eq!(tamano(313_890), "313,8 kB");
    assert_eq!(tamano(1_048_576), "1,0 MB");
    assert_eq!(fecha(0, 5), "—");
    // 13/11/2026 10:04 UTC.
    let t = 1_794_564_240;
    assert_eq!(fecha(t, t + 60), "10:04");
    assert_eq!(fecha(t, t + 86_400 * 3), "13 nov");
    assert_eq!(fecha(t, t + 86_400 * 400), "13 nov 2026");
    assert_eq!(texto_seleccion(&["nota.txt"], 18, 0), "\"nota.txt\" elegido (18 bytes)");
    assert_eq!(texto_seleccion(&["a", "b"], 2000, 0), "2 elementos elegidos (2,0 kB)");
}

// ---------------------------------------------------------------------------------------------------------------
// Con el servicio falso.

#[test]
fn listar_no_ensena_lo_raro() {
    let mut f = Falso::nuevo();
    let c = f.asas[&RAIZ_HOME].0;
    f.anadir(c, "raro|nombre", false, b"x", 1000, 0o644);
    f.anadir(c, "tab\tnombre", false, b"x", 1000, 0o644);
    let (v, raros) = listar(&mut f, RAIZ_HOME, 1000).unwrap();
    assert_eq!(raros, 2);
    assert!(v.iter().all(|e| nombre_valido(&e.nombre)));
    assert_eq!(v.len(), 3);
}

#[test]
fn lo_de_carla_no_se_ve() {
    let mut f = Falso::nuevo();
    // Se llega al cajón (la x de la raíz), pero no se lista ni se entra en él.
    let r = abrir(&mut f, &Lugar { raiz: RAIZ_SISTEMA, nombres: vec!["DeCarla".into()] }).unwrap();
    assert_eq!(listar(&mut f, r, 100).err(), Some(a::PROTEGIDO));
    cerrar(&mut f, r);
    let mut t = Trabajo::nuevo(Tarea::Copiar, vec![el(Lugar { raiz: RAIZ_SISTEMA, nombres: vec!["DeCarla".into()] }, "secreto.txt", false)], Some(home(&[])), None, vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert!(f.en("C/secreto.txt").is_none());
    assert_eq!(t.errores.len(), 1, "{:?}", t.errores);
}

#[test]
fn abrir_no_sale_de_la_raiz() {
    let mut f = Falso::nuevo();
    for malo in [vec![".."], vec!["Fotos", ".."], vec!["a/b"], vec![""]] {
        let l = Lugar { raiz: RAIZ_HOME, nombres: malo.iter().map(|x| String::from(*x)).collect() };
        assert_eq!(abrir(&mut f, &l), Err(a::MAL_FORMADO));
    }
    // Una raíz que no tiene: asa mala.
    let r = abrir(&mut f, &Lugar::raiz(9)).unwrap();
    assert_eq!(listar(&mut f, r, 10).err(), Some(a::ASA_MALA));
    assert!(f.asas.keys().all(|&k| k < 16), "no deja asas abiertas");
}

#[test]
fn copiar_una_carpeta_entera_con_progreso() {
    let mut f = Falso::nuevo();
    let mut t = Trabajo::nuevo(Tarea::Copiar, vec![el(home(&[]), "Fotos", true), el(home(&[]), "nota.txt", false)], Some(home(&["Privado"])), None, vec![1, 2]);
    let mut visto = vec![];
    while !t.paso(&mut f, 2) {
        visto.push(t.milesimas());
    }
    assert!(t.errores.is_empty(), "{:?}", t.errores);
    assert_eq!(f.nombres("C/Privado"), ["Fotos", "carta.txt", "nota.txt"]);
    assert_eq!(f.en("C/Privado/Fotos/a.png").unwrap().datos, vec![7u8; 10_000]);
    assert_eq!(f.en("C/Privado/Fotos/b.png").unwrap().modo, 0o600, "se copian los permisos");
    assert_eq!(t.bytes_total, 15_018);
    assert!(visto.windows(2).all(|w| w[0] <= w[1]), "el progreso no va hacia atrás");
    assert!(visto.iter().any(|&m| m > 0 && m < 1000));
    assert!(f.asas.keys().all(|&k| k < 16), "no deja asas abiertas: {:?}", f.asas.keys());
}

#[test]
fn copiar_sobre_si_mismo_hace_otra() {
    let mut f = Falso::nuevo();
    let mut t = Trabajo::nuevo(Tarea::Copiar, vec![el(home(&[]), "nota.txt", false), el(home(&[]), "Fotos", true)], Some(home(&[])), None, vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert!(t.errores.is_empty(), "{:?}", t.errores);
    assert_eq!(f.en("C/nota (copia).txt").unwrap().datos, b"hola, soy una nota");
    assert_eq!(f.nombres("C/Fotos (copia)"), ["a.png", "b.png"]);
    assert_eq!(f.en("C/nota.txt").unwrap().datos, b"hola, soy una nota", "el original no se toca");
}

#[test]
fn ni_copiar_ni_mover_dentro_de_si_misma() {
    for clase in [Tarea::Copiar, Tarea::Mover] {
        let mut f = Falso::nuevo();
        let mut t = Trabajo::nuevo(clase, vec![el(home(&[]), "Fotos", true)], Some(home(&["Fotos"])), None, vec![1, 2]);
        hasta_el_final(&mut t, &mut f);
        assert_eq!(t.errores.len(), 1);
        assert_eq!(f.nombres("C/Fotos"), ["a.png", "b.png"]);
    }
    // Y aunque el explorador se equivocara, el servicio no deja moverla (por asas).
    let mut f = Falso::nuevo();
    let fotos = abrir(&mut f, &home(&["Fotos"])).unwrap();
    assert_eq!(renombrar(&mut f, RAIZ_HOME, "Fotos", fotos, "x"), a::MAL_FORMADO);
}

#[test]
fn disco_lleno_para_y_no_deja_medias_copias() {
    let mut f = Falso::nuevo();
    f.cabe = f.usado() + 12_000;
    // ventana de 4 KB: a.png va en tres trozos y no cabe dos veces.
    let mut t = Trabajo::nuevo(Tarea::Copiar, vec![el(home(&["Fotos"]), "a.png", false), el(home(&["Fotos"]), "b.png", false), el(home(&[]), "nota.txt", false)], Some(home(&[])), None, vec![1, 2]);
    // Primero cabe: a.png (10.000); luego b.png no (5.000).
    hasta_el_final(&mut t, &mut f);
    assert!(t.errores.iter().any(|e| e.contains("no queda sitio")), "{:?}", t.errores);
    assert!(f.en("C/a.png").is_some());
    assert!(f.en("C/b.png").is_none(), "lo copiado a medias se borra");
    assert!(f.en("C/nota.txt").is_some_and(|n| n.datos.len() == 18), "lo de después no se copia: {:?}", f.nombres("C"));
    assert!(f.asas.keys().all(|&k| k < 16));
}

#[test]
fn cancelar_a_medias() {
    let mut f = Falso::nuevo();
    let mut t = Trabajo::nuevo(Tarea::Copiar, vec![el(home(&["Fotos"]), "a.png", false)], Some(home(&["Privado"])), None, vec![1, 2]);
    // Hasta que va por el segundo trozo.
    while t.bytes == 0 {
        t.paso(&mut f, 1);
    }
    t.cancelar = true;
    assert!(t.paso(&mut f, 1));
    assert!(f.en("C/Privado/a.png").is_none(), "{:?}", f.nombres("C/Privado"));
    assert!(f.asas.keys().all(|&k| k < 16));
}

#[test]
fn por_sistema_no_se_escribe() {
    let mut f = Falso::nuevo();
    let mut t = Trabajo::nuevo(Tarea::Copiar, vec![el(home(&[]), "nota.txt", false)], Some(Lugar::raiz(RAIZ_SISTEMA)), None, vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert_eq!(t.errores.len(), 1);
    let mut t = Trabajo::nuevo(Tarea::Borrar, vec![el(Lugar { raiz: RAIZ_SISTEMA, nombres: vec!["Programas".into()] }, "hola", false)], None, None, vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert_eq!(t.errores.len(), 1);
    assert!(f.en("Programas/hola").is_some());
}

#[test]
fn mover_y_no_pisar() {
    let mut f = Falso::nuevo();
    let c = f.asas[&RAIZ_HOME].0;
    let p = f.hijo(c, "Privado").unwrap();
    f.anadir(p, "nota.txt", false, b"otra", 1000, 0o644);
    let mut t = Trabajo::nuevo(Tarea::Mover, vec![el(home(&[]), "nota.txt", false), el(home(&[]), "Fotos", true)], Some(home(&["Privado"])), None, vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert_eq!(t.errores.len(), 1, "{:?}", t.errores);
    assert_eq!(f.en("C/Privado/nota.txt").unwrap().datos, b"otra", "no se pisa");
    assert!(f.en("C/nota.txt").is_some());
    assert!(f.en("C/Privado/Fotos/a.png").is_some());
    assert!(f.en("C/Fotos").is_none());
}

#[test]
fn papelera_tirar_restaurar_y_vaciar() {
    let mut f = Falso::nuevo();
    let pap = con_papelera(&mut f);
    let mut t = Trabajo::nuevo(Tarea::Papelera, vec![el(home(&[]), "nota.txt", false), el(home(&[]), "Fotos", true)], None, Some(pap.clone()), vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert!(t.errores.is_empty(), "{:?}", t.errores);
    assert_eq!(f.nombres("C/.Papelera/archivos"), ["Fotos", "nota.txt"]);
    assert_eq!(f.nombres("C/.Papelera/info"), ["Fotos", "nota.txt"]);
    // Otra nota con el mismo nombre: no pisa.
    let c = f.asas[&RAIZ_HOME].0;
    f.anadir(c, "nota.txt", false, b"segunda", 1000, 0o644);
    let mut t = Trabajo::nuevo(Tarea::Papelera, vec![el(home(&[]), "nota.txt", false)], None, Some(pap.clone()), vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert_eq!(f.nombres("C/.Papelera/archivos"), ["Fotos", "nota (2).txt", "nota.txt"]);
    // Restaurar la primera: vuelve a su sitio, y como ya hay otra nota... no: la segunda está en la papelera.
    let archivos = pap.hijo(PAPELERA_ARCHIVOS);
    let mut t = Trabajo::nuevo(Tarea::Restaurar, vec![el(archivos.clone(), "nota.txt", false), el(archivos.clone(), "Fotos", true)], None, Some(pap.clone()), vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert!(t.errores.is_empty(), "{:?}", t.errores);
    assert_eq!(f.en("C/nota.txt").unwrap().datos, b"hola, soy una nota");
    assert_eq!(f.nombres("C/Fotos"), ["a.png", "b.png"]);
    assert_eq!(f.nombres("C/.Papelera/info"), ["nota (2).txt"]);
    // Restaurar la segunda: ya hay una nota.txt, así que vuelve con otro nombre.
    let mut t = Trabajo::nuevo(Tarea::Restaurar, vec![el(archivos.clone(), "nota (2).txt", false)], None, Some(pap.clone()), vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert_eq!(f.en("C/nota (restaurado).txt").unwrap().datos, b"segunda");
    // Vaciar: borrar de verdad lo que haya (una carpeta con cosas dentro también).
    let mut t = Trabajo::nuevo(Tarea::Papelera, vec![el(home(&[]), "Fotos", true)], None, Some(pap.clone()), vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    let todo: Vec<Elemento> = f.nombres("C/.Papelera/archivos").iter().map(|n| el(archivos.clone(), n, n == "Fotos")).collect();
    let mut t = Trabajo::nuevo(Tarea::Borrar, todo, None, Some(pap.clone()), vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert!(t.errores.is_empty(), "{:?}", t.errores);
    assert!(f.nombres("C/.Papelera/archivos").is_empty());
    assert!(f.nombres("C/.Papelera/info").is_empty());
    assert!(f.asas.keys().all(|&k| k < 16));
}

#[test]
fn restaurar_con_ficha_retocada_no_sale_de_la_raiz() {
    let mut f = Falso::nuevo();
    let pap = con_papelera(&mut f);
    let c = f.asas[&RAIZ_HOME].0;
    let p = f.hijo(c, PAPELERA).unwrap();
    let ar = f.hijo(p, PAPELERA_ARCHIVOS).unwrap();
    let inf = f.hijo(p, PAPELERA_INFO).unwrap();
    f.anadir(ar, "x", false, b"x", 1000, 0o644);
    // La ficha dice "vuelve a ../../DeCarla" y a la raíz de Sistema (que no se le da).
    f.anadir(inf, "x", false, b"dashos-papelera 1\nraiz 2\nnombre x\ncajon ..\ncajon DeCarla\n", 1000, 0o644);
    f.anadir(ar, "y", false, b"y", 1000, 0o644);
    f.anadir(inf, "y", false, b"dashos-papelera 1\nraiz 1\nnombre y\ncajon DeCarla\n", 1000, 0o644);
    let archivos = pap.hijo(PAPELERA_ARCHIVOS);
    let mut t = Trabajo::nuevo(Tarea::Restaurar, vec![el(archivos.clone(), "x", false), el(archivos, "y", false)], None, Some(pap), vec![2]);
    hasta_el_final(&mut t, &mut f);
    assert_eq!(t.errores.len(), 2, "{:?}", t.errores);
    assert_eq!(f.nombres("C/.Papelera/archivos"), ["x", "y"]);
    assert_eq!(f.nombres("DeCarla"), ["secreto.txt"]);
}

#[test]
fn borrar_de_verdad_una_carpeta_llena() {
    let mut f = Falso::nuevo();
    let mut t = Trabajo::nuevo(Tarea::Borrar, vec![el(home(&[]), "Fotos", true)], None, None, vec![1, 2]);
    hasta_el_final(&mut t, &mut f);
    assert!(t.errores.is_empty(), "{:?}", t.errores);
    assert!(f.en("C/Fotos").is_none());
}

#[test]
fn buscar_en_las_subcarpetas() {
    let mut f = Falso::nuevo();
    let mut b = Busqueda::nueva("PNG", home(&[]), false);
    while !b.acabada {
        b.paso(&mut f, 2);
    }
    let v: Vec<String> = b.encontrado.iter().map(|(l, e)| alloc::format!("{}", l.hijo(&e.nombre).texto("Home"))).collect();
    assert_eq!(v, ["Home/Fotos/a.png", "Home/Fotos/b.png"]);
    assert!(f.asas.keys().all(|&k| k < 16));
}

#[test]
fn mensajes_al_azar_no_lo_rompen() {
    // Entradas y fichas al azar contra la lógica: nada entra en pánico.
    let mut x: u64 = 0x1234_5678;
    let mut azar = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..2000 {
        let n = (azar() % 40) as usize;
        let bytes: Vec<u8> = (0..n).map(|_| (azar() % 256) as u8).collect();
        let _ = Ficha::leer(&bytes, &[1, 2]);
        let t = String::from_utf8_lossy(&bytes);
        let _ = interpretar_camino(&t, &[("Home", 2)], &home(&[]));
        let _ = nombre_valido(&t);
        let mut s = Seleccion::nueva(n);
        s.clic((azar() % 50) as usize, azar() % 2 == 0, azar() % 2 == 0);
        let _ = mover_cursor(Some((azar() % 60) as usize), Paso::Abajo, n, (azar() % 5) as usize, 3);
    }
}
