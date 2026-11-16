// This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of the MPL was not
// distributed with this file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! El intérprete del firmware del PHY de la Realtek RTL8125 (X5c): el formato de `rtl8125b-2.fw` y sus acciones, escrito
//! con la especificación de sala limpia `docs/spec/hw/rtl8125.md`, sección 6.3 (las secciones se citan como "§"). Nada
//! de Linux ni de BSD.
//!
//! No toca el aparato: el controlador (`rtl-red`) le da un [`Bus`] con los accesos de OCP al PHY y a la MAC (§3.1,
//! §3.2) y las esperas. Así se prueba en el PC con un aparato falso (`rustc --test`, en `make -C kernel prueba`).
//!
//! Primero se valida el fichero entero (cabecera, suma, códigos, saltos y cambios de modo); solo si todo vale se
//! ejecuta. Los saltos se validan como Linux (§6.3, "Validación exacta"); además, más prudente que Linux y con las
//! respuestas del analista (§17), son error un `PHY_BJMPN` de 0 (se repetiría siempre igual), un acceso en modo PHY a
//! un registro por debajo de 0x10 con una página distinta de la estándar, una lectura del "registro 0x1F" en modo MAC
//! y una dirección OCP impar o mayor de 0xFFFF (el firmware real no usa ninguno). Y un tope de acciones ejecutadas,
//! para que un bucle de espera del fichero no cuelgue el controlador.

/// La base de la página estándar del PHY (§3.2): el registro MII `r` está en `0xA400 + 2·r`.
pub const BASE_ESTANDAR: u16 = 0xA400;

/// Tope de acciones ejecutadas (con los saltos hacia atrás, una acción cuenta cada vez). El fichero de la 8125B tiene
/// 800 acciones; sus bucles de espera, si el aparato no contesta, no deben colgar el controlador.
pub const TOPE_PASOS: u32 = 200_000;

/// Los accesos que necesita el intérprete. Las direcciones son de OCP (pares, de 16 bits).
pub trait Bus {
    /// Leer un registro del PHY por OCP (§3.2). `None` si no contesta a tiempo.
    fn phy_leer(&mut self, dir: u16) -> Option<u16>;
    /// Escribir un registro del PHY por OCP. Falso si no termina a tiempo.
    fn phy_escribir(&mut self, dir: u16, v: u16) -> bool;
    /// Leer un registro de la MAC por OCP (§3.1).
    fn mac_leer(&mut self, dir: u16) -> u16;
    /// Escribir un registro de la MAC por OCP.
    fn mac_escribir(&mut self, dir: u16, v: u16);
    /// Esperar `ms` milisegundos.
    fn esperar_ms(&mut self, ms: u32);
}

/// Lo que puede ir mal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallo {
    /// El fichero está vacío o no llega a una acción.
    Vacio,
    /// Con cabecera: menos bytes que la cabecera, versión sin cero, o `inicio`/`n` fuera del fichero.
    Cabecera,
    /// La suma de todos los bytes no da 0.
    Suma,
    /// Sin cabecera, el tamaño no es múltiplo de 4.
    Tamano,
    /// La acción `i` tiene un código desconocido.
    Codigo(usize),
    /// La acción `i` salta fuera de la lista (o un `PHY_BJMPN` de 0).
    Salto(usize),
    /// La acción `i` es un `PHY_MDIO_CHG` con un dato mayor que 1.
    Modo(usize),
    /// Al ejecutar la acción `i`: una dirección que no vale (impar, mayor de 0xFFFF, o un caso que la especificación no
    /// cubre).
    Direccion(usize),
    /// Al ejecutar la acción `i`: el PHY no contesta.
    Phy(usize),
    /// Se pasó el tope de acciones ejecutadas.
    Tope,
}

/// Un fichero validado.
#[derive(Debug, Clone, Copy)]
pub struct Firmware<'a> {
    /// La versión de la cabecera (sin el cero), si la tiene.
    pub version: Option<&'a [u8]>,
    /// Las acciones: `n` palabras de 32 bits en little-endian.
    acciones: &'a [u8],
}

/// Lo que dejó una ejecución.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resumen {
    /// Acciones ejecutadas (con las repetidas).
    pub pasos: u32,
    /// Escrituras hechas (al PHY o a la MAC, sin contar los cambios de base).
    pub escrituras: u32,
}

// Los códigos (§6.3).
const PHY_READ: u32 = 0x0;
const PHY_DATA_OR: u32 = 0x1;
const PHY_DATA_AND: u32 = 0x2;
const PHY_BJMPN: u32 = 0x3;
const PHY_MDIO_CHG: u32 = 0x4;
const PHY_CLEAR_READCOUNT: u32 = 0x7;
const PHY_WRITE: u32 = 0x8;
const PHY_READCOUNT_EQ_SKIP: u32 = 0x9;
const PHY_COMP_EQ_SKIPN: u32 = 0xA;
const PHY_COMP_NEQ_SKIPN: u32 = 0xB;
const PHY_WRITE_PREVIOUS: u32 = 0xC;
const PHY_SKIPN: u32 = 0xD;
const PHY_DELAY_MS: u32 = 0xE;

/// Tamaño de la cabecera: marca (4), versión (32), inicio (4), n (4) y suma (1).
const CABECERA: usize = 45;

fn palabra(d: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]])
}

/// Valida el fichero entero (§6.3) antes de ejecutar nada.
pub fn validar(datos: &[u8]) -> Result<Firmware<'_>, Fallo> {
    if datos.len() < 4 {
        return Err(Fallo::Vacio);
    }
    let (version, acciones) = if palabra(datos, 0) == 0 {
        if datos.len() < CABECERA {
            return Err(Fallo::Cabecera);
        }
        if datos.iter().fold(0u8, |s, &b| s.wrapping_add(b)) != 0 {
            return Err(Fallo::Suma);
        }
        let texto = &datos[4..36];
        let fin = texto.iter().position(|&b| b == 0).ok_or(Fallo::Cabecera)?;
        let inicio = palabra(datos, 36) as usize;
        let n = palabra(datos, 40) as usize;
        // Las acciones, detrás de la cabecera y dentro del fichero.
        if inicio < CABECERA || inicio > datos.len() || n > (datos.len() - inicio) / 4 {
            return Err(Fallo::Cabecera);
        }
        (Some(&texto[..fin]), &datos[inicio..inicio + 4 * n])
    } else {
        // El formato antiguo: el fichero entero son acciones.
        if datos.len() % 4 != 0 {
            return Err(Fallo::Tamano);
        }
        (None, datos)
    };
    let n = acciones.len() / 4;
    if n == 0 {
        return Err(Fallo::Vacio);
    }
    for i in 0..n {
        let a = palabra(acciones, 4 * i);
        let (codigo, r, d) = (a >> 28, ((a >> 16) & 0xFFF) as usize, a & 0xFFFF);
        match codigo {
            PHY_READ | PHY_DATA_OR | PHY_DATA_AND | PHY_CLEAR_READCOUNT | PHY_WRITE | PHY_WRITE_PREVIOUS | PHY_DELAY_MS => {}
            PHY_MDIO_CHG => {
                if d > 1 {
                    return Err(Fallo::Modo(i));
                }
            }
            PHY_BJMPN => {
                if r == 0 || r > i {
                    return Err(Fallo::Salto(i));
                }
            }
            // Como Linux (§6.3, "Validación exacta"): saltarse la siguiente pide que haya otra detrás de ella.
            PHY_READCOUNT_EQ_SKIP => {
                if i + 2 >= n {
                    return Err(Fallo::Salto(i));
                }
            }
            // Saltarse las `r` siguientes: tiene que caer dentro de la lista; acabar justo al final no vale (como Linux).
            PHY_COMP_EQ_SKIPN | PHY_COMP_NEQ_SKIPN | PHY_SKIPN => {
                if i + 1 + r >= n {
                    return Err(Fallo::Salto(i));
                }
            }
            _ => return Err(Fallo::Codigo(i)),
        }
    }
    Ok(Firmware { version, acciones })
}

/// El estado del intérprete que toca el aparato: el modo y la base `B`, la misma en los dos modos (§6.3).
struct Acceso<'b, B: Bus> {
    bus: &'b mut B,
    mac: bool,
    base: u16,
}

impl<B: Bus> Acceso<'_, B> {
    /// La dirección OCP del registro `r` con la base de ahora. En modo PHY, la del registro MII con páginas (§3.2); en
    /// modo MAC, `B + r` (§6.3).
    fn direccion(&self, r: u32) -> Option<u16> {
        let d = if self.mac {
            self.base as u32 + r
        } else if self.base == BASE_ESTANDAR {
            BASE_ESTANDAR as u32 + 2 * r
        } else {
            // Con otra página, solo los registros de 0x10 en adelante (la especificación no dice qué pasa por debajo).
            self.base as u32 + 2 * r.checked_sub(0x10)?
        };
        (d & 1 == 0 && d <= 0xFFFF).then_some(d as u16)
    }

    fn leer(&mut self, r: u32, i: usize) -> Result<u16, Fallo> {
        if r == 0x1F {
            // En modo PHY, la página de ahora (sin tocar el aparato). En modo MAC la especificación no lo dice.
            if self.mac {
                return Err(Fallo::Direccion(i));
            }
            return Ok(if self.base == BASE_ESTANDAR { 0 } else { self.base >> 4 });
        }
        let d = self.direccion(r).ok_or(Fallo::Direccion(i))?;
        if self.mac { Ok(self.bus.mac_leer(d)) } else { self.bus.phy_leer(d).ok_or(Fallo::Phy(i)) }
    }

    /// Escribir `v` en `r`. Verdad si fue al aparato (no un cambio de base).
    fn escribir(&mut self, r: u32, v: u16, i: usize) -> Result<bool, Fallo> {
        if r == 0x1F {
            self.base = if self.mac || v != 0 { v << 4 } else { BASE_ESTANDAR };
            return Ok(false);
        }
        let d = self.direccion(r).ok_or(Fallo::Direccion(i))?;
        if self.mac {
            self.bus.mac_escribir(d, v);
        } else if !self.bus.phy_escribir(d, v) {
            return Err(Fallo::Phy(i));
        }
        Ok(true)
    }
}

impl Firmware<'_> {
    /// Número de acciones.
    pub fn acciones(&self) -> usize {
        self.acciones.len() / 4
    }

    /// Ejecuta las acciones (§6.3). Empieza en modo PHY con la base estándar y, pase lo que pase, la deja en la
    /// estándar al acabar. No espera al reinicio del PHY: eso lo hace el controlador.
    pub fn ejecutar<B: Bus>(&self, bus: &mut B) -> Result<Resumen, Fallo> {
        // La base vive solo aquí: al acabar, quien acceda al PHY vuelve a la estándar aunque el fichero no la devuelva
        // (§6.3, "al menos un fichero no la devuelve").
        self.correr(&mut Acceso { bus, mac: false, base: BASE_ESTANDAR })
    }

    fn correr<B: Bus>(&self, a: &mut Acceso<'_, B>) -> Result<Resumen, Fallo> {
        let n = self.acciones();
        let (mut previo, mut cuenta) = (0u16, 0u32);
        let mut res = Resumen { pasos: 0, escrituras: 0 };
        let mut i = 0usize;
        while i < n {
            res.pasos += 1;
            if res.pasos > TOPE_PASOS {
                return Err(Fallo::Tope);
            }
            let w = palabra(self.acciones, 4 * i);
            let (codigo, r, d) = (w >> 28, (w >> 16) & 0xFFF, (w & 0xFFFF) as u16);
            let mut siguiente = i + 1;
            match codigo {
                PHY_READ => {
                    previo = a.leer(r, i)?;
                    cuenta += 1;
                }
                PHY_DATA_OR => previo |= d,
                PHY_DATA_AND => previo &= d,
                PHY_BJMPN => siguiente = i - r as usize,
                PHY_MDIO_CHG => a.mac = d == 1,
                PHY_CLEAR_READCOUNT => cuenta = 0,
                PHY_WRITE => res.escrituras += a.escribir(r, d, i)? as u32,
                PHY_READCOUNT_EQ_SKIP => {
                    if cuenta == d as u32 {
                        siguiente += 1;
                    }
                }
                PHY_COMP_EQ_SKIPN => {
                    if previo == d {
                        siguiente += r as usize;
                    }
                }
                PHY_COMP_NEQ_SKIPN => {
                    if previo != d {
                        siguiente += r as usize;
                    }
                }
                PHY_WRITE_PREVIOUS => res.escrituras += a.escribir(r, previo, i)? as u32,
                PHY_SKIPN => siguiente += r as usize,
                PHY_DELAY_MS => a.bus.esperar_ms(d as u32),
                // `validar` ya no deja pasar otros códigos.
                _ => return Err(Fallo::Codigo(i)),
            }
            i = siguiente;
        }
        Ok(res)
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use std::collections::HashMap;
    use std::vec::Vec;

    /// Un aparato falso: guarda lo escrito y apunta cada acceso.
    #[derive(Default)]
    struct Falso {
        phy: HashMap<u16, u16>,
        mac: HashMap<u16, u16>,
        lecturas_phy: HashMap<u16, Vec<u16>>,
        diario: Vec<(char, u16, u16)>,
        esperado: u32,
        mudo: bool,
    }

    impl Bus for Falso {
        fn phy_leer(&mut self, dir: u16) -> Option<u16> {
            if self.mudo {
                return None;
            }
            // Si hay una lista de lecturas preparada, la siguiente; si no, lo escrito.
            let v = match self.lecturas_phy.get_mut(&dir) {
                Some(l) if !l.is_empty() => l.remove(0),
                _ => *self.phy.get(&dir).unwrap_or(&0),
            };
            self.diario.push(('r', dir, v));
            Some(v)
        }
        fn phy_escribir(&mut self, dir: u16, v: u16) -> bool {
            self.phy.insert(dir, v);
            self.diario.push(('w', dir, v));
            true
        }
        fn mac_leer(&mut self, dir: u16) -> u16 {
            let v = *self.mac.get(&dir).unwrap_or(&0);
            self.diario.push(('R', dir, v));
            v
        }
        fn mac_escribir(&mut self, dir: u16, v: u16) {
            self.mac.insert(dir, v);
            self.diario.push(('W', dir, v));
        }
        fn esperar_ms(&mut self, ms: u32) {
            self.esperado += ms;
        }
    }

    fn accion(codigo: u32, r: u32, d: u32) -> u32 {
        codigo << 28 | r << 16 | d
    }

    /// Un fichero con cabecera: versión, acciones y la suma que deja el total en 0.
    fn con_cabecera(version: &str, acciones: &[u32]) -> Vec<u8> {
        let inicio = 48usize;
        let mut f = vec![0u8; inicio];
        f[4..4 + version.len()].copy_from_slice(version.as_bytes());
        f[36..40].copy_from_slice(&(inicio as u32).to_le_bytes());
        f[40..44].copy_from_slice(&(acciones.len() as u32).to_le_bytes());
        for a in acciones {
            f.extend_from_slice(&a.to_le_bytes());
        }
        let suma = f.iter().fold(0u8, |s, &b| s.wrapping_add(b));
        f[44] = 0u8.wrapping_sub(suma);
        f
    }

    fn sin_cabecera(acciones: &[u32]) -> Vec<u8> {
        acciones.iter().flat_map(|a| a.to_le_bytes()).collect()
    }

    #[test]
    fn cabecera_y_version() {
        let f = con_cabecera("prueba_0.0.1 01/01/26", &[accion(PHY_WRITE, 0, 0x1000)]);
        let fw = validar(&f).unwrap();
        assert_eq!(fw.version, Some(&b"prueba_0.0.1 01/01/26"[..]));
        assert_eq!(fw.acciones(), 1);
    }

    #[test]
    fn suma_mal() {
        let mut f = con_cabecera("v", &[accion(PHY_WRITE, 0, 0x1000)]);
        f[44] = f[44].wrapping_add(1);
        assert_eq!(validar(&f).unwrap_err(), Fallo::Suma);
    }

    #[test]
    fn cabecera_fuera() {
        // `n` mayor de lo que cabe detrás de `inicio`.
        let mut f = con_cabecera("v", &[accion(PHY_WRITE, 0, 1)]);
        f[40] = 2;
        f[44] = f[44].wrapping_sub(1);
        assert_eq!(validar(&f).unwrap_err(), Fallo::Cabecera);
        // `inicio` más allá del fichero.
        let mut f = con_cabecera("v", &[accion(PHY_WRITE, 0, 1)]);
        f[36] = 200;
        f[44] = f[44].wrapping_sub(200 - 48);
        assert_eq!(validar(&f).unwrap_err(), Fallo::Cabecera);
        // Demasiado corto para la cabecera.
        assert_eq!(validar(&[0, 0, 0, 0, 1]).unwrap_err(), Fallo::Cabecera);
        // Sin el cero de la versión.
        let mut f = con_cabecera("", &[accion(PHY_WRITE, 0, 1)]);
        for b in &mut f[4..36] {
            *b = b'x';
        }
        f[44] = 0;
        let s = f.iter().fold(0u8, |s, &b| s.wrapping_add(b));
        f[44] = 0u8.wrapping_sub(s);
        assert_eq!(validar(&f).unwrap_err(), Fallo::Cabecera);
    }

    #[test]
    fn formato_antiguo() {
        let f = sin_cabecera(&[accion(PHY_WRITE, 0, 0x1000), accion(PHY_WRITE, 4, 0x01E1)]);
        let fw = validar(&f).unwrap();
        assert_eq!(fw.version, None);
        let mut b = Falso::default();
        assert_eq!(fw.ejecutar(&mut b).unwrap(), Resumen { pasos: 2, escrituras: 2 });
        assert_eq!(b.phy[&0xA400], 0x1000);
        assert_eq!(b.phy[&0xA408], 0x01E1);
        let mut f = f;
        f.push(0);
        assert_eq!(validar(&f).unwrap_err(), Fallo::Tamano);
        assert_eq!(validar(&[]).unwrap_err(), Fallo::Vacio);
    }

    #[test]
    fn codigos_y_saltos_invalidos() {
        // Códigos sin nombre.
        for c in [0x5, 0x6, 0xF] {
            assert_eq!(validar(&con_cabecera("v", &[accion(PHY_WRITE, 0, 1), accion(c, 0, 0)])).unwrap_err(), Fallo::Codigo(1));
        }
        // Atrás más allá del principio, y un salto de 0.
        assert_eq!(validar(&con_cabecera("v", &[accion(PHY_READ, 1, 0), accion(PHY_BJMPN, 2, 0)])).unwrap_err(), Fallo::Salto(1));
        assert_eq!(validar(&con_cabecera("v", &[accion(PHY_BJMPN, 0, 0)])).unwrap_err(), Fallo::Salto(0));
        // Adelante más allá del final, o justo hasta el final (inválido para Linux, §6.3); dentro de la lista sí vale.
        let w = accion(PHY_WRITE, 0, 1);
        assert_eq!(validar(&con_cabecera("v", &[accion(PHY_SKIPN, 2, 0), w])).unwrap_err(), Fallo::Salto(0));
        for c in [PHY_SKIPN, PHY_COMP_EQ_SKIPN, PHY_COMP_NEQ_SKIPN] {
            assert_eq!(validar(&con_cabecera("v", &[accion(c, 1, 0), w])).unwrap_err(), Fallo::Salto(0));
            assert_eq!(validar(&con_cabecera("v", &[w, accion(c, 2, 0), w, w])).unwrap_err(), Fallo::Salto(1));
            assert!(validar(&con_cabecera("v", &[accion(c, 1, 0), w, w])).is_ok());
        }
        assert_eq!(validar(&con_cabecera("v", &[w, accion(PHY_READCOUNT_EQ_SKIP, 0, 1)])).unwrap_err(), Fallo::Salto(1));
        assert_eq!(validar(&con_cabecera("v", &[accion(PHY_READCOUNT_EQ_SKIP, 0, 1), w])).unwrap_err(), Fallo::Salto(0));
        assert!(validar(&con_cabecera("v", &[accion(PHY_READCOUNT_EQ_SKIP, 0, 1), w, w])).is_ok());
        // Cambio de modo con un dato que no es 0 ni 1.
        assert_eq!(validar(&con_cabecera("v", &[accion(PHY_MDIO_CHG, 0, 2)])).unwrap_err(), Fallo::Modo(0));
        // Lo malo al final invalida también lo bueno de antes: no se ejecuta nada.
        let f = con_cabecera("v", &[accion(PHY_WRITE, 0, 1), accion(0xF, 0, 0)]);
        assert!(validar(&f).is_err());
    }

    #[test]
    fn paginas_del_phy() {
        // Página 0xA43, registro 0x10: OCP 0xA430 (§3.2). Leer 0x1F da la página; la página 0 vuelve a la estándar.
        let f = con_cabecera(
            "v",
            &[
                accion(PHY_WRITE, 0x1F, 0x0A43),
                accion(PHY_WRITE, 0x10, 0x1234),
                accion(PHY_READ, 0x1F, 0),
                accion(PHY_WRITE_PREVIOUS, 0x11, 0),
                accion(PHY_WRITE, 0x1F, 0),
                accion(PHY_WRITE, 0x02, 0x5678),
                accion(PHY_READ, 0x1F, 0),
                accion(PHY_WRITE_PREVIOUS, 0x03, 0),
            ],
        );
        let mut b = Falso::default();
        let r = validar(&f).unwrap().ejecutar(&mut b).unwrap();
        assert_eq!(b.phy[&0xA430], 0x1234);
        assert_eq!(b.phy[&0xA432], 0x0A43);
        assert_eq!(b.phy[&0xA404], 0x5678);
        assert_eq!(b.phy[&0xA406], 0);
        // Los cambios de página no van al aparato.
        assert_eq!(r.escrituras, 4);
        assert!(b.diario.iter().all(|&(_, d, _)| d != 0xA43E));
    }

    #[test]
    fn modo_mac_y_base_compartida() {
        // En modo MAC, 0x1F pone la base y el registro se suma sin multiplicar; la base sigue al volver a modo PHY.
        let f = con_cabecera(
            "v",
            &[
                accion(PHY_MDIO_CHG, 0, 1),
                accion(PHY_WRITE, 0x1F, 0x0F80),
                accion(PHY_WRITE, 0x02, 0xE008),
                accion(PHY_READ, 0x04, 0),
                accion(PHY_DATA_OR, 0, 0x0001),
                accion(PHY_WRITE_PREVIOUS, 0x06, 0),
                accion(PHY_WRITE, 0x1F, 0x0FC2),
                accion(PHY_WRITE, 0x08, 0x00AA),
                accion(PHY_MDIO_CHG, 0, 0),
                // Base 0xFC20 en modo PHY: el registro 0x10 es 0xFC20 + 0.
                accion(PHY_WRITE, 0x10, 0x0BB0),
            ],
        );
        let mut b = Falso::default();
        b.mac.insert(0xF804, 0x8000);
        validar(&f).unwrap().ejecutar(&mut b).unwrap();
        assert_eq!(b.mac[&0xF802], 0xE008);
        assert_eq!(b.mac[&0xF806], 0x8001);
        assert_eq!(b.mac[&0xFC28], 0x00AA);
        assert_eq!(b.phy[&0xFC20], 0x0BB0);
        // Una dirección impar en modo MAC no vale.
        let f = con_cabecera("v", &[accion(PHY_MDIO_CHG, 0, 1), accion(PHY_WRITE, 0x1F, 0x0F80), accion(PHY_WRITE, 0x03, 1)]);
        assert_eq!(validar(&f).unwrap().ejecutar(&mut Falso::default()).unwrap_err(), Fallo::Direccion(2));
        // Ni leer 0x1F en modo MAC.
        let f = con_cabecera("v", &[accion(PHY_MDIO_CHG, 0, 1), accion(PHY_READ, 0x1F, 0)]);
        assert_eq!(validar(&f).unwrap().ejecutar(&mut Falso::default()).unwrap_err(), Fallo::Direccion(1));
        // Ni un registro por debajo de 0x10 con otra página en modo PHY.
        let f = con_cabecera("v", &[accion(PHY_WRITE, 0x1F, 0x0B82), accion(PHY_WRITE, 0x05, 1)]);
        assert_eq!(validar(&f).unwrap().ejecutar(&mut Falso::default()).unwrap_err(), Fallo::Direccion(1));
    }

    #[test]
    fn bucle_de_espera() {
        // El patrón de un parche: pedir (0xB820 bit 4), esperar a que 0xB800 tenga el bit 6, con un tope de lecturas.
        let f = con_cabecera(
            "v",
            &[
                accion(PHY_WRITE, 0x1F, 0x0B82),
                accion(PHY_WRITE, 0x10, 0x0010),
                accion(PHY_WRITE, 0x1F, 0x0B80),
                accion(PHY_CLEAR_READCOUNT, 0, 0),
                // 4: leer, quedarse con el bit 6.
                accion(PHY_READ, 0x10, 0),
                accion(PHY_DATA_AND, 0, 0x0040),
                // Si ya está, saltarse las 4 siguientes.
                accion(PHY_COMP_EQ_SKIPN, 4, 0x0040),
                // Si van 3 lecturas, saltarse el salto atrás y rendirse.
                accion(PHY_READCOUNT_EQ_SKIP, 0, 3),
                accion(PHY_SKIPN, 1, 0),
                accion(PHY_SKIPN, 1, 0),
                accion(PHY_BJMPN, 6, 0),
                // 11: esperar 5 ms y marcar el fin.
                accion(PHY_DELAY_MS, 0, 5),
                accion(PHY_WRITE, 0x11, 0x00FF),
            ],
        );
        let fw = validar(&f).unwrap();
        let mut b = Falso::default();
        b.lecturas_phy.insert(0xB800, vec![0, 0, 0x0040]);
        let r = fw.ejecutar(&mut b).unwrap();
        assert_eq!(b.esperado, 5);
        assert_eq!(b.phy[&0xB802], 0x00FF);
        assert_eq!(b.diario.iter().filter(|&&(c, d, _)| c == 'r' && d == 0xB800).count(), 3);
        assert_eq!(r.escrituras, 2);
        // Si el bit no llega, el contador de lecturas corta el bucle.
        let mut b = Falso::default();
        fw.ejecutar(&mut b).unwrap();
        assert_eq!(b.diario.iter().filter(|&&(c, d, _)| c == 'r' && d == 0xB800).count(), 3);
        // Un bucle sin salida acaba en el tope.
        let f = con_cabecera(
            "v",
            &[accion(PHY_READ, 0, 0), accion(PHY_COMP_NEQ_SKIPN, 1, 0), accion(PHY_BJMPN, 2, 0), accion(PHY_WRITE, 0, 1)],
        );
        assert_eq!(validar(&f).unwrap().ejecutar(&mut Falso::default()).unwrap_err(), Fallo::Tope);
    }

    #[test]
    fn el_phy_no_contesta() {
        let f = con_cabecera("v", &[accion(PHY_WRITE, 0, 1), accion(PHY_READ, 1, 0)]);
        let mut b = Falso { mudo: true, ..Default::default() };
        assert_eq!(validar(&f).unwrap().ejecutar(&mut b).unwrap_err(), Fallo::Phy(1));
    }
}
