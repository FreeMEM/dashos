//! Los menús: `CreateMenusA`, `FreeMenus`, `LayoutMenusA` y `LayoutMenuItemsA` de gadtools.library, `ItemAddress`,
//! `OnMenu` y `OffMenu` de intuition, y lo que pasa con el botón derecho: la barra de menús sale en la barra de
//! título de la ventana (cada ventana Amiga es una ventana del escritorio: no hay barra de la pantalla), el menú cae
//! debajo, y al soltar llega `IDCMP_MENUPICK` con el número del elemento (`FULLMENUNUM`), o `MENUNULL`.
//!
//! Escrito desde los autodocs de gadtools e intuition, `intuition/intuition.h` y `libraries/gadtools.h` del NDK y el
//! RKRM (Libraries, "Intuition Menus"). El aspecto, el de los menús "NewLook" de 3.x: fondo de `BARBLOCKPEN`, texto de
//! `BARDETAILPEN` y lo elegido al revés.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Memoria};

use crate::acceso::Acceso;
use crate::estructuras::{drawinfo as dri, idcmp, window as win};
use crate::tema::{Elem, Titulo, VistaMenu};
use crate::lienzo::{Pinta, Rect};
use crate::{CajaGrafica, Compositor, Fallo, fuente};

/// `struct Menu` (con el `UserData` de GadTools detrás).
pub mod menu {
    pub const NEXTMENU: u32 = 0;
    pub const LEFTEDGE: u32 = 4;
    pub const TOPEDGE: u32 = 6;
    pub const WIDTH: u32 = 8;
    pub const HEIGHT: u32 = 10;
    pub const FLAGS: u32 = 12;
    pub const MENUNAME: u32 = 14;
    pub const FIRSTITEM: u32 = 18;
    pub const TAMANO: u32 = 30;
    pub const MENUENABLED: u16 = 1;
}

/// `struct MenuItem` (con el `UserData` de GadTools detrás).
pub mod item {
    pub const NEXTITEM: u32 = 0;
    pub const LEFTEDGE: u32 = 4;
    pub const TOPEDGE: u32 = 6;
    pub const WIDTH: u32 = 8;
    pub const HEIGHT: u32 = 10;
    pub const FLAGS: u32 = 12;
    pub const MUTUALEXCLUDE: u32 = 14;
    pub const ITEMFILL: u32 = 18;
    pub const COMMAND: u32 = 26;
    pub const SUBITEM: u32 = 28;
    pub const NEXTSELECT: u32 = 32;
    pub const TAMANO: u32 = 34;
    pub const CHECKIT: u16 = 1;
    pub const ITEMTEXT: u16 = 2;
    pub const COMMSEQ: u16 = 4;
    pub const MENUTOGGLE: u16 = 8;
    pub const ITEMENABLED: u16 = 0x10;
    pub const HIGHCOMP: u16 = 0x40;
    pub const CHECKED: u16 = 0x100;
}

/// `struct IntuiText`.
pub mod itext {
    pub const FRONTPEN: u32 = 0;
    pub const DRAWMODE: u32 = 2;
    pub const LEFTEDGE: u32 = 4;
    pub const TOPEDGE: u32 = 6;
    pub const ITEXT: u32 = 12;
    pub const TAMANO: u32 = 20;
}

/// `struct NewMenu`.
mod newmenu {
    pub const TYPE: u32 = 0;
    pub const LABEL: u32 = 2;
    pub const COMMKEY: u32 = 6;
    pub const FLAGS: u32 = 10;
    pub const MUTUALEXCLUDE: u32 = 12;
    pub const USERDATA: u32 = 16;
    pub const TAMANO: u32 = 20;
    pub const NM_TITLE: u8 = 1;
    pub const NM_ITEM: u8 = 2;
    pub const NM_SUB: u8 = 3;
    pub const NM_IGNORE: u8 = 64;
}

pub const MENUNULL: u16 = 0xFFFF;
const NOITEM: u16 = 0x3F;
const NOSUB: u16 = 0x1F;
/// Lo que ocupa la marca de `CHECKIT` a la izquierda y la tecla Amiga a la derecha.
const ANCHO_MARCA: i32 = 15;
pub(crate) const ANCHO_AMIGA: i32 = 23;
/// Tope de menús y elementos que se recorren (más: una lista en círculo).
const MAX_ELEMENTOS: u32 = 4096;

/// Un menú abierto con el botón derecho.
pub(crate) struct Abierto {
    pub w: u32,
    pub tira: u32,
    pub menu: i32,
    pub item: i32,
    /// Lo que había debajo (el lienzo entero de la ventana).
    pub debajo: Vec<u32>,
}

#[derive(Default)]
pub(crate) struct Estado {
    /// Menús de `CreateMenusA`: el primero → (tarea, bloques).
    pub creados: BTreeMap<u32, (u32, Vec<(u32, u32)>)>,
    pub abierto: Option<Abierto>,
}

fn siguientes<M: Memoria>(ex: &Exec<M>, primero: u32, campo: u32) -> Vec<u32> {
    let mut v = Vec::new();
    let mut d = primero;
    while d != 0 && (v.len() as u32) < MAX_ELEMENTOS {
        v.push(d);
        d = ex.l32(d + campo).unwrap_or(0);
    }
    v
}

impl<C: Compositor> CajaGrafica<C> {
    /// `CreateMenusA(newmenu, tags)`: los `struct Menu` y `struct MenuItem` de la tabla, cada uno con su `UserData`
    /// detrás (`GTMENU_USERDATA`). Devuelve el primer menú, o 0.
    pub(crate) fn create_menus<M: Memoria>(&mut self, ex: &mut Exec<M>, nm: u32, _tags: u32) -> Result<u32, Fallo> {
        let mut bloques: Vec<(u32, u32)> = Vec::new();
        let nuevo = |ex: &mut Exec<M>, n: u32, bloques: &mut Vec<(u32, u32)>| -> Result<u32, Fallo> {
            let d = ex.alloc_mem(n, memf::PUBLIC | memf::CLEAR)?;
            if d == 0 {
                return Err(Fallo::SinMemoria);
            }
            bloques.push((d, n));
            Ok(d)
        };
        let (mut primero, mut menu, mut item, mut sub) = (0u32, 0u32, 0u32, 0u32);
        let mut i = 0;
        let r = (|| -> Result<(), Fallo> {
            loop {
                if i >= MAX_ELEMENTOS {
                    return Err(Fallo::Sospechoso { dir: nm });
                }
                let e = nm + i * newmenu::TAMANO;
                i += 1;
                let tipo = ex.l8(e.wrapping_add(newmenu::TYPE))?;
                if tipo == 0 {
                    return Ok(());
                }
                if tipo & newmenu::NM_IGNORE != 0 {
                    continue;
                }
                let (etiq, tecla, flags, mx, ud) = (
                    ex.l32(e.wrapping_add(newmenu::LABEL))?,
                    ex.l32(e.wrapping_add(newmenu::COMMKEY))?,
                    ex.l16(e.wrapping_add(newmenu::FLAGS))?,
                    ex.l32(e.wrapping_add(newmenu::MUTUALEXCLUDE))?,
                    ex.l32(e.wrapping_add(newmenu::USERDATA))?,
                );
                match tipo {
                    newmenu::NM_TITLE => {
                        let m = nuevo(ex, menu::TAMANO + 4, &mut bloques)?;
                        ex.e32(m.wrapping_add(menu::MENUNAME), etiq)?;
                        ex.e16(m.wrapping_add(menu::FLAGS), if flags & menu::MENUENABLED != 0 { 0 } else { menu::MENUENABLED })?;
                        ex.e32(m.wrapping_add(menu::TAMANO), ud)?;
                        if menu == 0 {
                            primero = m;
                        } else {
                            ex.e32(menu.wrapping_add(menu::NEXTMENU), m)?;
                        }
                        (menu, item, sub) = (m, 0, 0);
                    }
                    newmenu::NM_ITEM | newmenu::NM_SUB => {
                        if menu == 0 || (tipo == newmenu::NM_SUB && item == 0) {
                            return Err(Fallo::Sospechoso { dir: e });
                        }
                        let it = nuevo(ex, item::TAMANO + 4, &mut bloques)?;
                        let mut f;
                        if etiq == u32::MAX {
                            // NM_BARLABEL: una raya (sin texto, sin poder elegirse).
                            f = 0;
                        } else {
                            let t = nuevo(ex, itext::TAMANO, &mut bloques)?;
                            ex.e8(t.wrapping_add(itext::FRONTPEN), self.intui.wb.plumas[dri::BARDETAILPEN] as u8)?;
                            ex.e8(t.wrapping_add(itext::DRAWMODE), 1)?;
                            ex.e16(t.wrapping_add(itext::TOPEDGE), 1)?;
                            ex.e32(t.wrapping_add(itext::ITEXT), etiq)?;
                            ex.e32(it.wrapping_add(item::ITEMFILL), t)?;
                            f = item::ITEMTEXT | item::HIGHCOMP;
                            if flags & item::ITEMENABLED == 0 {
                                f |= item::ITEMENABLED;
                            }
                            f |= flags & (item::CHECKIT | item::CHECKED | item::MENUTOGGLE);
                            if tecla != 0 {
                                let k = ex.l8(tecla)?;
                                if k != 0 {
                                    ex.e8(it.wrapping_add(item::COMMAND), k)?;
                                    f |= item::COMMSEQ;
                                }
                            }
                        }
                        ex.e16(it.wrapping_add(item::FLAGS), f)?;
                        ex.e32(it.wrapping_add(item::MUTUALEXCLUDE), mx)?;
                        ex.e32(it.wrapping_add(item::TAMANO), ud)?;
                        if tipo == newmenu::NM_ITEM {
                            if item == 0 {
                                ex.e32(menu.wrapping_add(menu::FIRSTITEM), it)?;
                            } else {
                                ex.e32(item.wrapping_add(item::NEXTITEM), it)?;
                            }
                            (item, sub) = (it, 0);
                        } else {
                            if sub == 0 {
                                ex.e32(item.wrapping_add(item::SUBITEM), it)?;
                            } else {
                                ex.e32(sub.wrapping_add(item::NEXTITEM), it)?;
                            }
                            sub = it;
                        }
                    }
                    _ => return Err(Fallo::Sospechoso { dir: e }),
                }
            }
        })();
        if let Err(f) = r {
            for (d, n) in bloques {
                ex.free_mem(d, n)?;
            }
            return Err(f);
        }
        if primero == 0 {
            for (d, n) in bloques {
                ex.free_mem(d, n)?;
            }
            return Ok(0);
        }
        self.menus.creados.insert(primero, (ex.tarea_actual(), bloques));
        Ok(primero)
    }

    pub(crate) fn free_menus<M: Memoria>(&mut self, ex: &mut Exec<M>, m: u32) -> Result<(), Fallo> {
        if m == 0 {
            return Ok(());
        }
        let Some((_, bloques)) = self.menus.creados.remove(&m) else { return Err(Fallo::Sospechoso { dir: m }) };
        if self.menus.abierto.as_ref().is_some_and(|a| a.tira == m) {
            self.cerrar_menu(ex, None);
        }
        for (d, n) in bloques {
            ex.free_mem(d, n)?;
        }
        Ok(())
    }

    pub(crate) fn menus_tarea_muerta(&mut self, t: u32) {
        self.menus.creados.retain(|_, (x, _)| *x != t);
        self.menus.abierto = None;
    }

    pub(crate) fn menu_ventana_cerrada(&mut self, w: u32) {
        if self.menus.abierto.as_ref().is_some_and(|a| a.w == w) {
            self.menus.abierto = None;
        }
    }

    /// El ancho de un elemento: su texto, la marca y la tecla.
    fn ancho_item<M: Memoria>(ex: &Exec<M>, it: u32) -> i32 {
        let f = ex.l16(it.wrapping_add(item::FLAGS)).unwrap_or(0);
        let mut an = 0;
        if f & item::ITEMTEXT != 0 {
            let t = ex.l32(it.wrapping_add(item::ITEMFILL)).unwrap_or(0);
            let s = ex.l32(t.wrapping_add(itext::ITEXT)).unwrap_or(0);
            an = fuente::ANCHO as i32 * ex.cadena(s).map(|v| v.len()).unwrap_or(0) as i32;
        }
        if f & item::CHECKIT != 0 {
            an += ANCHO_MARCA;
        }
        if f & item::COMMSEQ != 0 {
            an += ANCHO_AMIGA + fuente::ANCHO as i32 + 8;
        }
        an
    }

    /// Coloca los elementos de una lista: uno debajo de otro, todos del ancho del más ancho; los subelementos, a la
    /// derecha de su elemento.
    /// Coloca una lista de elementos y, si no es ya de subelementos (`sub`), sus subelementos: en intuition no hay más
    /// niveles, y un `SubItem` que vuelve a una lista de antes no debe hacer dar vueltas sin fin.
    fn colocar_items<M: Memoria>(&mut self, ex: &mut Exec<M>, primero: u32, x: i32, sub: bool) -> Result<(), Fallo> {
        let items = siguientes(ex, primero, item::NEXTITEM);
        let ancho = items.iter().map(|&i| Self::ancho_item(ex, i)).max().unwrap_or(0) + 8;
        let mut y = 2;
        for &it in &items {
            let f = ex.l16(it.wrapping_add(item::FLAGS))?;
            let alto = if f & item::ITEMTEXT != 0 { fuente::ALTO as i32 + 2 } else { 6 };
            ex.e16(it.wrapping_add(item::LEFTEDGE), x as u16)?;
            ex.e16(it.wrapping_add(item::TOPEDGE), y as u16)?;
            ex.e16(it.wrapping_add(item::WIDTH), ancho as u16)?;
            ex.e16(it.wrapping_add(item::HEIGHT), alto as u16)?;
            if f & item::ITEMTEXT != 0 {
                let t = ex.l32(it.wrapping_add(item::ITEMFILL))?;
                ex.e16(t.wrapping_add(itext::LEFTEDGE), if f & item::CHECKIT != 0 { ANCHO_MARCA as u16 } else { 2 })?;
            }
            let s = ex.l32(it.wrapping_add(item::SUBITEM))?;
            if s != 0 && !sub {
                self.colocar_items(ex, s, ancho - 4, true)?;
            }
            y += alto;
        }
        Ok(())
    }

    /// `LayoutMenusA(menu, vi, tags)` (y `LayoutMenuItemsA` con `solo_items`): pone posiciones y tamaños.
    pub(crate) fn layout_menus<M: Memoria>(&mut self, ex: &mut Exec<M>, m: u32, solo_items: bool) -> Result<u32, Fallo> {
        if m == 0 {
            return Ok(0);
        }
        if solo_items {
            self.colocar_items(ex, m, 0, false)?;
            return Ok(1);
        }
        let mut x = 5;
        for mu in siguientes(ex, m, menu::NEXTMENU) {
            let nombre = ex.cadena(ex.l32(mu.wrapping_add(menu::MENUNAME))?).unwrap_or_default();
            let ancho = fuente::ANCHO as i32 * nombre.len() as i32 + 8;
            ex.e16(mu.wrapping_add(menu::LEFTEDGE), x as u16)?;
            ex.e16(mu.wrapping_add(menu::TOPEDGE), 0)?;
            ex.e16(mu.wrapping_add(menu::WIDTH), ancho as u16)?;
            ex.e16(mu.wrapping_add(menu::HEIGHT), fuente::ALTO as u16 + 2)?;
            let primero = ex.l32(mu.wrapping_add(menu::FIRSTITEM))?;
            self.colocar_items(ex, primero, 0, false)?;
            x += ancho;
        }
        Ok(1)
    }

    /// `ItemAddress(menustrip, number)`: el elemento (o subelemento) de ese número, o 0.
    pub(crate) fn item_address<M: Memoria>(&self, ex: &Exec<M>, tira: u32, n: u32) -> Result<u32, Fallo> {
        let n = n as u16;
        if n == MENUNULL || tira == 0 {
            return Ok(0);
        }
        let (m, i, s) = (n & 0x1F, (n >> 5) & 0x3F, (n >> 11) & 0x1F);
        let Some(&mu) = siguientes(ex, tira, menu::NEXTMENU).get(m as usize) else { return Ok(0) };
        if i == NOITEM {
            return Ok(0);
        }
        let Some(&it) = siguientes(ex, ex.l32(mu.wrapping_add(menu::FIRSTITEM))?, item::NEXTITEM).get(i as usize) else { return Ok(0) };
        if s == NOSUB {
            return Ok(it);
        }
        Ok(siguientes(ex, ex.l32(it.wrapping_add(item::SUBITEM))?, item::NEXTITEM).get(s as usize).copied().unwrap_or(0))
    }

    /// `OnMenu`/`OffMenu(window, number)`: el menú entero (sin elemento) o un elemento.
    pub(crate) fn on_off_menu<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, n: u32, si: bool) -> Result<(), Fallo> {
        self.ventana(w)?;
        let tira = ex.l32(w.wrapping_add(win::MENUSTRIP))?;
        let n16 = n as u16;
        if (n16 >> 5) & 0x3F == NOITEM {
            if let Some(&mu) = siguientes(ex, tira, menu::NEXTMENU).get((n16 & 0x1F) as usize) {
                let f = ex.l16(mu.wrapping_add(menu::FLAGS))?;
                ex.e16(mu.wrapping_add(menu::FLAGS), if si { f | menu::MENUENABLED } else { f & !menu::MENUENABLED })?;
            }
            return Ok(());
        }
        let it = self.item_address(ex, tira, n)?;
        if it != 0 {
            let f = ex.l16(it.wrapping_add(item::FLAGS))?;
            ex.e16(it.wrapping_add(item::FLAGS), if si { f | item::ITEMENABLED } else { f & !item::ITEMENABLED })?;
        }
        Ok(())
    }

    // --- Con el botón derecho ---

    /// El botón derecho bajó en la ventana `w`: si tiene menús, se abre la barra. `false` si no tiene.
    pub(crate) fn abrir_menu<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32) -> bool {
        let tira = ex.l32(w.wrapping_add(win::MENUSTRIP)).unwrap_or(0);
        if tira == 0 || self.menus.abierto.is_some() {
            return false;
        }
        let Some(bm) = self.intui.ventanas.get(&w).map(|v| v.bm) else { return false };
        let Some(mut l) = crate::graphics::vista(&mut self.gfx.lienzos, ex, bm) else { return false };
        let l = &mut l;
        let r = l.rect();
        let debajo = l.copiar(r);
        self.menus.abierto = Some(Abierto { w, tira, menu: -1, item: -1, debajo });
        // Lo que ya está bajo el ratón (el título sobre el que se pulsó).
        let x = ex.l16(w.wrapping_add(win::MOUSEX)).unwrap_or(0) as i16 as i32;
        let y = ex.l16(w.wrapping_add(win::MOUSEY)).unwrap_or(0) as i16 as i32;
        self.mover_menu(ex, w, x, y);
        self.dibujar_menu(ex);
        true
    }

    /// El ratón se movió con el menú abierto: qué título y qué elemento quedan señalados.
    pub(crate) fn mover_menu<M: Memoria>(&mut self, ex: &mut Exec<M>, w: u32, x: i32, y: i32) -> bool {
        let Some(a) = self.menus.abierto.as_ref() else { return false };
        if a.w != w {
            return true;
        }
        let tira = a.tira;
        let alto_barra = self.intui.ventanas.get(&w).map_or(11, |v| v.geometria.bordes[1].max(fuente::ALTO as i32 + 3));
        let menus = siguientes(ex, tira, menu::NEXTMENU);
        let (mut m, mut i) = (a.menu, -1);
        if y < alto_barra {
            m = menus
                .iter()
                .position(|&mu| {
                    let x0 = ex.l16(mu.wrapping_add(menu::LEFTEDGE)).unwrap_or(0) as i16 as i32;
                    let an = ex.l16(mu.wrapping_add(menu::WIDTH)).unwrap_or(0) as i16 as i32;
                    x >= x0 && x < x0 + an
                })
                .map_or(m, |p| p as i32);
        } else if let Some(&mu) = menus.get(m.max(0) as usize).filter(|_| m >= 0) {
            let x0 = ex.l16(mu.wrapping_add(menu::LEFTEDGE)).unwrap_or(0) as i16 as i32;
            for (k, &it) in siguientes(ex, ex.l32(mu.wrapping_add(menu::FIRSTITEM)).unwrap_or(0), item::NEXTITEM).iter().enumerate() {
                let ix = x0 + ex.l16(it.wrapping_add(item::LEFTEDGE)).unwrap_or(0) as i16 as i32;
                let iy = alto_barra + ex.l16(it.wrapping_add(item::TOPEDGE)).unwrap_or(0) as i16 as i32;
                let an = ex.l16(it.wrapping_add(item::WIDTH)).unwrap_or(0) as i16 as i32;
                let al = ex.l16(it.wrapping_add(item::HEIGHT)).unwrap_or(0) as i16 as i32;
                if x >= ix && x < ix + an && y >= iy && y < iy + al {
                    i = k as i32;
                }
            }
        }
        let a = self.menus.abierto.as_mut().unwrap();
        if (a.menu, a.item) != (m, i) {
            (a.menu, a.item) = (m, i);
            self.dibujar_menu(ex);
        }
        true
    }

    /// El botón derecho subió: se cierra y, si había un elemento elegible señalado, `IDCMP_MENUPICK` con su número
    /// (si no, con `MENUNULL`). Devuelve si hay que replanificar; `None` si no había menú abierto.
    pub(crate) fn soltar_menu<M: Memoria>(&mut self, ex: &mut Exec<M>) -> Option<Result<bool, Fallo>> {
        let a = self.menus.abierto.as_ref()?;
        let (w, tira, m, i) = (a.w, a.tira, a.menu, a.item);
        let mut codigo = MENUNULL;
        if m >= 0 && i >= 0 {
            let n = (m as u16 & 0x1F) | ((i as u16 & 0x3F) << 5) | (NOSUB << 11);
            let it = self.item_address(ex, tira, n as u32).unwrap_or(0);
            let mu = siguientes(ex, tira, menu::NEXTMENU).get(m as usize).copied().unwrap_or(0);
            let f = ex.l16(it.wrapping_add(item::FLAGS)).unwrap_or(0);
            let menu_bien = ex.l16(mu.wrapping_add(menu::FLAGS)).unwrap_or(0) & menu::MENUENABLED != 0;
            if it != 0 && menu_bien && f & item::ITEMENABLED != 0 {
                codigo = n;
                if f & item::CHECKIT != 0 {
                    let nf = if f & item::MENUTOGGLE != 0 { f ^ item::CHECKED } else { f | item::CHECKED };
                    let _ = ex.e16(it.wrapping_add(item::FLAGS), nf);
                }
                let _ = ex.e16(it.wrapping_add(item::NEXTSELECT), MENUNULL);
            }
        }
        self.cerrar_menu(ex, Some(w));
        Some(self.enviar(ex, w, idcmp::MENUPICK, codigo, 0))
    }

    /// Quita el menú de la ventana (vuelve lo que había debajo).
    fn cerrar_menu<M: Memoria>(&mut self, ex: &mut Exec<M>, _w: Option<u32>) {
        let Some(a) = self.menus.abierto.take() else { return };
        let Some(bm) = self.intui.ventanas.get(&a.w).map(|v| v.bm) else { return };
        let Some(mut l) = crate::graphics::vista(&mut self.gfx.lienzos, ex, bm) else { return };
        let l = &mut l;
        let an = l.ancho() as i32;
        if a.debajo.len() == (l.ancho() * l.alto()) as usize {
            for (k, &p) in a.debajo.iter().enumerate() {
                l.poner(k as i32 % an, k as i32 / an, p);
            }
        }
        let r = l.rect();
        l.danar(r);
    }

    /// Dibuja la barra y el menú señalado sobre lo que había debajo.
    fn dibujar_menu<M: Memoria>(&mut self, ex: &mut Exec<M>) {
        let Some(a) = self.menus.abierto.as_ref() else { return };
        let (w, tira, msel, isel) = (a.w, a.tira, a.menu, a.item);
        let Some(v) = self.intui.ventanas.get(&w) else { return };
        let (bm, alto_barra) = (v.bm, v.geometria.bordes[1].max(fuente::ALTO as i32 + 3));
        let plumas = self.plumas(ex, w);
        let tema = self.tema_de(w);
        let mut titulos = Vec::new();
        let mut elems = Vec::new();
        let mut caja = None;
        for (k, mu) in siguientes(ex, tira, menu::NEXTMENU).into_iter().enumerate() {
            let x = ex.l16(mu.wrapping_add(menu::LEFTEDGE)).unwrap_or(0) as i16 as i32;
            let an = ex.l16(mu.wrapping_add(menu::WIDTH)).unwrap_or(0) as i16 as i32;
            let nombre = ex.cadena(ex.l32(mu.wrapping_add(menu::MENUNAME)).unwrap_or(0)).unwrap_or_default();
            let activo_m = ex.l16(mu.wrapping_add(menu::FLAGS)).unwrap_or(0) & menu::MENUENABLED != 0;
            titulos.push(Titulo { x, an, nombre, activo: activo_m });
            if k as i32 != msel {
                continue;
            }
            let items = siguientes(ex, ex.l32(mu.wrapping_add(menu::FIRSTITEM)).unwrap_or(0), item::NEXTITEM);
            let mut total = Rect::nuevo(x, alto_barra, x, alto_barra);
            for (j, &it) in items.iter().enumerate() {
                let ix = x + ex.l16(it.wrapping_add(item::LEFTEDGE)).unwrap_or(0) as i16 as i32;
                let iy = alto_barra + ex.l16(it.wrapping_add(item::TOPEDGE)).unwrap_or(0) as i16 as i32;
                let an = ex.l16(it.wrapping_add(item::WIDTH)).unwrap_or(0) as i16 as i32;
                let al = ex.l16(it.wrapping_add(item::HEIGHT)).unwrap_or(0) as i16 as i32;
                let f = ex.l16(it.wrapping_add(item::FLAGS)).unwrap_or(0);
                let r = Rect::nuevo(ix, iy, ix + an - 1, iy + al - 1);
                total = total.union(&r);
                let fill = ex.l32(it.wrapping_add(item::ITEMFILL)).unwrap_or(0);
                let (texto_i, tx) = if f & item::ITEMTEXT != 0 && fill != 0 {
                    (ex.cadena(ex.l32(fill.wrapping_add(itext::ITEXT)).unwrap_or(0)).unwrap_or_default(), ex.l16(fill.wrapping_add(itext::LEFTEDGE)).unwrap_or(0) as i16 as i32)
                } else {
                    (Vec::new(), 0)
                };
                elems.push(Elem {
                    r,
                    texto: texto_i,
                    tx,
                    raya: f & item::ITEMTEXT == 0 && fill == 0,
                    activo: j as i32 == isel && f & item::ITEMENABLED != 0 && activo_m,
                    marca: (f & item::CHECKIT != 0).then_some(f & item::CHECKED != 0),
                    tecla: (f & item::COMMSEQ != 0).then(|| ex.l8(it.wrapping_add(item::COMMAND)).unwrap_or(0)),
                });
            }
            if !items.is_empty() {
                caja = Some(Rect::nuevo(total.x0 - 2, total.y0, total.x1 + 2, total.y1 + 2));
            }
        }
        let glifos = &self.intui.glifos;
        let Some(mut l) = crate::graphics::vista(&mut self.gfx.lienzos, ex, bm) else { return };
        let l = &mut l;
        let Some(a) = self.menus.abierto.as_ref() else { return };
        let an_l = l.ancho() as i32;
        if a.debajo.len() == (l.ancho() * l.alto()) as usize {
            for (k, &p) in a.debajo.iter().enumerate() {
                l.poner(k as i32 % an_l, k as i32 / an_l, p);
            }
        }
        let vista = VistaMenu { alto_barra, ancho: an_l, titulos: &titulos, elems: &elems, caja, msel };
        tema.como().menu(l, glifos, &plumas, &vista);
        let r = l.rect();
        l.danar(r);
    }
}
