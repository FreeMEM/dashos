//! La tabla de llamadas: número de función nativa → función de graphics o de intuition, con los registros del
//! `.fd`. Un fallo se apunta como aviso y la función devuelve lo que devolvería al fallar (0, o -1 en las que
//! lo dicen sus autodocs), sin tumbar la caja. Lo que aún no está hecho da el aviso `NoImplementada` y D0 = 0.

use caja_exec::{Accion, Exec, Memoria, Registros};

use crate::acceso::{Acceso, palabra};
use crate::estructuras::{rastport as rp_, window as win};
use crate::lvo::{GRAPHICS as G, INTUITION as I, fila};
use crate::{CajaGrafica, Compositor, Fallo};

macro_rules! filas {
    ($tabla:ident; $($c:ident = $n:literal),* $(,)?) => { $(const $c: usize = fila(&$tabla, $n);)* };
}

filas!(G;
    BLTBITMAP = "BltBitMap", BLTTEMPLATE = "BltTemplate", CLEAREOL = "ClearEOL", CLEARSCREEN = "ClearScreen",
    TEXTLENGTH = "TextLength", TEXT = "Text", SETFONT = "SetFont", OPENFONT = "OpenFont", CLOSEFONT = "CloseFont",
    ASKSOFTSTYLE = "AskSoftStyle", SETSOFTSTYLE = "SetSoftStyle", LOADRGB4 = "LoadRGB4",
    INITRASTPORT = "InitRastPort", MOVE = "Move", DRAW = "Draw", RECTFILL = "RectFill", WRITEPIXEL = "WritePixel",
    READPIXEL = "ReadPixel", SETRAST = "SetRast", SCROLLRASTER = "ScrollRaster", SCROLLRASTERBF = "ScrollRasterBF",
    SETAPEN = "SetAPen", SETBPEN = "SetBPen", SETDRMD = "SetDrMd", SETRGB4 = "SetRGB4", INITBITMAP = "InitBitMap",
    ALLOCRASTER = "AllocRaster", FREERASTER = "FreeRaster", ASKFONT = "AskFont", GETRGB4 = "GetRGB4",
    POLYDRAW = "PolyDraw", BLTCLEAR = "BltClear", CLIPBLIT = "ClipBlit", WAITTOF = "WaitTOF", WAITBLIT = "WaitBlit",
    OWNBLITTER = "OwnBlitter", DISOWNBLITTER = "DisownBlitter", GETCOLORMAP = "GetColorMap",
    FREECOLORMAP = "FreeColorMap", BLTBITMAPRASTPORT = "BltBitMapRastPort", SETRGB4CM = "SetRGB4CM",
    SETRGB32 = "SetRGB32", GETRGB32 = "GetRGB32", LOADRGB32 = "LoadRGB32", SETRGB32CM = "SetRGB32CM",
    SETABPENDRMD = "SetABPenDrMd", GETAPEN = "GetAPen", GETBPEN = "GetBPen", GETDRMD = "GetDrMd",
    GETOUTLINEPEN = "GetOutlinePen", SETOUTLINEPEN = "SetOutlinePen", SETWRITEMASK = "SetWriteMask",
    SETMAXPEN = "SetMaxPen", ALLOCBITMAP = "AllocBitMap", FREEBITMAP = "FreeBitMap", GETBITMAPATTR = "GetBitMapAttr",
    RELEASEPEN = "ReleasePen", OBTAINPEN = "ObtainPen", OBTAINBESTPENA = "ObtainBestPenA", FINDCOLOR = "FindColor",
    WRITECHUNKYPIXELS = "WriteChunkyPixels", VBEAMPOS = "VBeamPos", GETVPMODEID = "GetVPModeID",
);

filas!(I;
    OPENWINDOW = "OpenWindow", CLOSEWINDOW = "CloseWindow", OPENWINDOWTAGLIST = "OpenWindowTagList",
    REFRESHWINDOWFRAME = "RefreshWindowFrame", SETWINDOWTITLES = "SetWindowTitles", MODIFYIDCMP = "ModifyIDCMP",
    ACTIVATEWINDOW = "ActivateWindow", WINDOWTOFRONT = "WindowToFront", WINDOWTOBACK = "WindowToBack",
    MOVEWINDOW = "MoveWindow", SIZEWINDOW = "SizeWindow", CHANGEWINDOWBOX = "ChangeWindowBox",
    WINDOWLIMITS = "WindowLimits", BEGINREFRESH = "BeginRefresh", ENDREFRESH = "EndRefresh",
    LOCKPUBSCREEN = "LockPubScreen", UNLOCKPUBSCREEN = "UnlockPubScreen", GETSCREENDATA = "GetScreenData",
    GETSCREENDRAWINFO = "GetScreenDrawInfo", FREESCREENDRAWINFO = "FreeScreenDrawInfo",
    VIEWPORTADDRESS = "ViewPortAddress", CURRENTTIME = "CurrentTime", DISPLAYBEEP = "DisplayBeep",
    SETMENUSTRIP = "SetMenuStrip", CLEARMENUSTRIP = "ClearMenuStrip", RESETMENUSTRIP = "ResetMenuStrip",
    SETPOINTER = "SetPointer", CLEARPOINTER = "ClearPointer", SETWINDOWPOINTERA = "SetWindowPointerA",
    SCREENTOFRONT = "ScreenToFront", SCREENTOBACK = "ScreenToBack",
    ADDGADGET = "AddGadget", REMOVEGADGET = "RemoveGadget", ADDGLIST = "AddGList", REMOVEGLIST = "RemoveGList",
    REFRESHGLIST = "RefreshGList", REFRESHGADGETS = "RefreshGadgets", ONGADGET = "OnGadget", OFFGADGET = "OffGadget",
    ITEMADDRESS = "ItemAddress", ONMENU = "OnMenu", OFFMENU = "OffMenu", OPENSCREENTAGLIST = "OpenScreenTagList",
    OPENSCREEN = "OpenScreen", CLOSESCREEN = "CloseScreen", GETDEFAULTPUBSCREEN = "GetDefaultPubScreen",
    INTUITEXTLENGTH = "IntuiTextLength", EASYREQUESTARGS = "EasyRequestArgs",
);

/// Las funciones que devuelven -1 al fallar.
fn falla_con_menos_uno(lib_gfx: bool, f: usize) -> bool {
    lib_gfx && matches!(f, READPIXEL | WRITEPIXEL | OBTAINPEN | OBTAINBESTPENA | GETRGB4)
}

impl<C: Compositor> CajaGrafica<C> {
    /// Llama a la función nativa `n` con los registros de la tarea actual. Es lo que hace la caja cuando exec
    /// devuelve `Accion::Ajena` y `n` es de graphics o de intuition; con otro número devuelve `Accion::Ajena`.
    /// La caja no tiene que repetir ninguna: no bloquean (el `Accion::Ceder` es por haber despertado a otra).
    pub fn llamar<M: Memoria>(&mut self, ex: &mut Exec<M>, n: u16, r: &mut Registros) -> Accion {
        let (g0, i0) = (self.numeros.graphics, self.numeros.intuition);
        if n == self.numeros.intuition + EASYREQUESTARGS as u16 {
            // Bloquea a la tarea hasta que pulsen un botón (requester.rs).
            return self.easy_request(ex, r);
        }
        if let Some(f) = self.es_gadtools(n) {
            match self.llamar_gadtools(ex, f, r) {
                Ok(v) => r.d[0] = v,
                Err(e) => {
                    self.avisar(ex, crate::lvo::GADTOOLS[f].0, e);
                    r.d[0] = 0;
                }
            }
            return Accion::Seguir;
        }
        if let Some((de_layers, f)) = self.es_capas(n) {
            let res = if de_layers { self.llamar_layers(ex, f, r) } else { self.llamar_keymap(ex, f, r) };
            match res {
                Ok(v) => r.d[0] = v,
                Err(e) => {
                    let nombre = if de_layers { crate::capas::LAYERS[f] } else { crate::capas::KEYMAP[f] };
                    self.avisar(ex, nombre, e);
                    r.d[0] = 0;
                }
            }
            return Accion::Seguir;
        }
        let (gfx, f) = if n >= g0 && ((n - g0) as usize) < G.len() {
            (true, (n - g0) as usize)
        } else if n >= i0 && ((n - i0) as usize) < I.len() {
            (false, (n - i0) as usize)
        } else {
            return Accion::Ajena;
        };
        let res = if gfx { self.llamar_graphics(ex, f, r) } else { self.llamar_intuition(ex, f, r) };
        match res {
            Ok(v) => {
                r.d[0] = v;
                Accion::Seguir
            }
            Err(e) => {
                let nombre = if gfx { G[f].0 } else { I[f].0 };
                self.avisar(ex, nombre, e);
                r.d[0] = if falla_con_menos_uno(gfx, f) { u32::MAX } else { 0 };
                Accion::Seguir
            }
        }
    }

    fn llamar_graphics<M: Memoria>(&mut self, ex: &mut Exec<M>, f: usize, r: &mut Registros) -> Result<u32, Fallo> {
        let (d, a) = (r.d, r.a);
        let w = |i: usize| palabra(d[i]);
        Ok(match f {
            BLTBITMAP => {
                self.blt_bit_map(ex, a[0], w(0), w(1), a[1], w(2), w(3), w(4), w(5), d[6] as u8, d[7] as u8)?
            }
            BLTTEMPLATE => {
                self.blt_template(ex, a[0], d[0], palabra(d[1]), a[1], w(2), w(3), w(4), w(5))?;
                0
            }
            CLEAREOL => {
                self.clear_eol(ex, a[1])?;
                0
            }
            CLEARSCREEN => {
                self.clear_screen(ex, a[1])?;
                0
            }
            TEXTLENGTH => self.text_length(ex, a[1], a[0], d[0])?,
            TEXT => {
                self.text(ex, a[1], a[0], d[0])?;
                0
            }
            SETFONT => {
                self.set_font(ex, a[1], a[0])?;
                0
            }
            OPENFONT => self.open_font(ex, a[0])?,
            CLOSEFONT => {
                self.close_font(ex, a[1])?;
                0
            }
            // Sin estilos algorítmicos todavía (negrita, cursiva, subrayado): ninguno disponible.
            ASKSOFTSTYLE | SETSOFTSTYLE => {
                self.leer_rp(ex, a[1])?;
                0
            }
            LOADRGB4 => {
                self.load_rgb4(ex, a[0], a[1], d[0])?;
                0
            }
            INITRASTPORT => {
                self.init_rast_port(ex, a[1])?;
                0
            }
            MOVE => {
                self.mover(ex, a[1], w(0), w(1))?;
                0
            }
            DRAW => {
                self.draw(ex, a[1], w(0), w(1))?;
                0
            }
            RECTFILL => {
                self.rect_fill(ex, a[1], w(0), w(1), w(2), w(3))?;
                0
            }
            WRITEPIXEL => self.write_pixel(ex, a[1], w(0), w(1))? as u32,
            READPIXEL => self.read_pixel(ex, a[1], w(0), w(1))? as u32,
            SETRAST => {
                self.set_rast(ex, a[1], d[0])?;
                0
            }
            SCROLLRASTER | SCROLLRASTERBF => {
                self.scroll_raster(ex, a[1], w(0), w(1), w(2), w(3), w(4), w(5))?;
                0
            }
            SETAPEN => {
                self.set_a_pen(ex, a[1], d[0])?;
                0
            }
            SETBPEN => {
                self.set_b_pen(ex, a[1], d[0])?;
                0
            }
            SETDRMD => {
                self.set_dr_md(ex, a[1], d[0])?;
                0
            }
            SETABPENDRMD => {
                self.set_a_pen(ex, a[1], d[0])?;
                self.set_b_pen(ex, a[1], d[1])?;
                self.set_dr_md(ex, a[1], d[2])?;
                0
            }
            GETAPEN | GETBPEN | GETDRMD | GETOUTLINEPEN => {
                let i = self.leer_rp(ex, a[0])?;
                match f {
                    GETAPEN => i.fg as u32,
                    GETBPEN => i.bg as u32,
                    GETDRMD => i.modo as u32,
                    _ => ex.l8(a[0] + rp_::AOLPEN)? as u32,
                }
            }
            SETOUTLINEPEN => self.set_outline_pen(ex, a[0], d[0])?,
            SETWRITEMASK => self.set_write_mask(ex, a[0], d[0])?,
            SETMAXPEN => {
                self.leer_rp(ex, a[0])?;
                0
            }
            SETRGB4 => {
                self.set_rgb4(ex, a[0], d[0], d[1], d[2], d[3])?;
                0
            }
            SETRGB4CM => {
                self.set_rgb4_cm(ex, a[0], d[0], d[1], d[2], d[3])?;
                0
            }
            SETRGB32 => {
                self.set_rgb32(ex, a[0], d[0], d[1], d[2], d[3])?;
                0
            }
            SETRGB32CM => {
                self.set_rgb32_cm(ex, a[0], d[0], d[1], d[2], d[3])?;
                0
            }
            LOADRGB32 => {
                self.load_rgb32(ex, a[0], a[1])?;
                0
            }
            GETRGB4 => self.get_rgb4(ex, a[0], d[0])? as u32,
            GETRGB32 => {
                self.get_rgb32(ex, a[0], d[0], d[1], a[1])?;
                0
            }
            GETCOLORMAP => self.get_color_map(ex, d[0])?,
            FREECOLORMAP => {
                self.free_color_map(ex, a[0])?;
                0
            }
            OBTAINPEN => self.obtain_pen(ex, a[0], d[0] as i32, d[1], d[2], d[3])? as u32,
            OBTAINBESTPENA => self.find_color(a[0], d[1], d[2], d[3], -1)?,
            FINDCOLOR => self.find_color(a[3], d[1], d[2], d[3], d[4] as i32)?,
            RELEASEPEN => {
                self.release_pen(a[0], d[0]);
                0
            }
            INITBITMAP => {
                self.init_bit_map(ex, a[0], d[0], d[1], d[2])?;
                0
            }
            ALLOCRASTER => self.alloc_raster(ex, d[0], d[1])?,
            FREERASTER => {
                self.free_raster(ex, a[0], d[0], d[1])?;
                0
            }
            ALLOCBITMAP => self.alloc_bit_map(ex, d[0], d[1], d[2], d[3], a[0])?,
            FREEBITMAP => {
                self.free_bit_map(ex, a[0])?;
                0
            }
            GETBITMAPATTR => self.get_bit_map_attr(ex, a[0], d[1])?,
            ASKFONT => {
                self.ask_font(ex, a[1], a[0])?;
                0
            }
            POLYDRAW => {
                self.poly_draw(ex, a[1], d[0], a[0])?;
                0
            }
            BLTCLEAR => {
                self.blt_clear(ex, a[1], d[0], d[1])?;
                0
            }
            CLIPBLIT => {
                self.clip_blit(ex, a[0], true, w(0), w(1), a[1], w(2), w(3), w(4), w(5), d[6] as u8)?;
                0
            }
            BLTBITMAPRASTPORT => {
                self.clip_blit(ex, a[0], false, w(0), w(1), a[1], w(2), w(3), w(4), w(5), d[6] as u8)?;
                0
            }
            WRITECHUNKYPIXELS => {
                self.write_chunky_pixels(ex, a[0], w(0), w(1), w(2), w(3), a[2], d[4] as i32)?;
                0
            }
            // No hay blitter ni barrido: no se espera a nada (provisional: WaitTOF debería esperar al cuadro).
            WAITTOF | WAITBLIT | OWNBLITTER | DISOWNBLITTER | VBEAMPOS => 0,
            // El modo de la pantalla: "PAL:HighRes" (PAL_MONITOR_ID | HIRES_KEY), el del Workbench de un A1200.
            GETVPMODEID => 0x0002_9000,
            _ => return Err(Fallo::NoImplementada { n: self.numeros.graphics + f as u16 }),
        })
    }

    fn llamar_intuition<M: Memoria>(&mut self, ex: &mut Exec<M>, f: usize, r: &mut Registros) -> Result<u32, Fallo> {
        let (d, a) = (r.d, r.a);
        let w = |i: usize| palabra(d[i]);
        let hecho = |res: Result<(), Fallo>| res.map(|_| 0u32);
        match f {
            OPENWINDOW => self.open_window_tag_list(ex, a[0], 0),
            OPENWINDOWTAGLIST => self.open_window_tag_list(ex, a[0], a[1]),
            CLOSEWINDOW => {
                if a[0] == 0 {
                    return Ok(0);
                }
                hecho(self.close_window(ex, a[0]))
            }
            REFRESHWINDOWFRAME => hecho(self.refresh_window_frame(a[0])),
            SETWINDOWTITLES => hecho(self.set_window_titles(ex, a[0], a[1], a[2])),
            MODIFYIDCMP => self.modify_idcmp(ex, a[0], d[0]),
            ACTIVATEWINDOW => hecho(self.activate_window(a[0])),
            WINDOWTOFRONT => hecho(self.window_to_front(a[0])),
            WINDOWTOBACK => hecho(self.window_to_back(a[0])),
            MOVEWINDOW => hecho(self.move_window(ex, a[0], w(0), w(1))),
            SIZEWINDOW => hecho(self.size_window(a[0], w(0), w(1))),
            CHANGEWINDOWBOX => hecho(self.change_window_box(a[0], w(0), w(1), w(2), w(3))),
            WINDOWLIMITS => self.window_limits(ex, a[0], w(0), w(1), w(2), w(3)),
            BEGINREFRESH => hecho(self.ventana(a[0]).map(|_| ())),
            ENDREFRESH => hecho(self.end_refresh(ex, a[0], d[0] != 0)),
            LOCKPUBSCREEN => self.lock_pub_screen(ex, a[0]),
            UNLOCKPUBSCREEN => hecho(self.unlock_pub_screen(ex, a[0], a[1])),
            GETSCREENDATA => self.get_screen_data(ex, a[0], d[0], d[1], a[1]),
            GETSCREENDRAWINFO => self.get_screen_draw_info(a[0]),
            FREESCREENDRAWINFO => Ok(0),
            VIEWPORTADDRESS => self.view_port_address(a[0]),
            CURRENTTIME => hecho(self.current_time(ex, a[0], a[1])),
            DISPLAYBEEP => {
                self.comp.pitido();
                Ok(0)
            }
            // Los menús salen con el botón derecho (menus.rs).
            SETMENUSTRIP | RESETMENUSTRIP => {
                self.ventana(a[0])?;
                ex.e32(a[0] + win::MENUSTRIP, a[1])?;
                Ok(1)
            }
            CLEARMENUSTRIP => {
                self.ventana(a[0])?;
                self.menu_ventana_cerrada(a[0]);
                ex.e32(a[0] + win::MENUSTRIP, 0)?;
                Ok(0)
            }
            // El puntero es el del escritorio (provisional).
            SETPOINTER | CLEARPOINTER | SETWINDOWPOINTERA => hecho(self.ventana(a[0]).map(|_| ())),
            SCREENTOFRONT | SCREENTOBACK => Ok(0),
            ADDGADGET => self.add_g_list(ex, a[0], a[1], palabra(d[0]), 1),
            ADDGLIST => self.add_g_list(ex, a[0], a[1], palabra(d[0]), palabra(d[1])),
            REMOVEGADGET => self.remove_g_list(ex, a[0], a[1], 1),
            REMOVEGLIST => self.remove_g_list(ex, a[0], a[1], palabra(d[0])),
            REFRESHGLIST => {
                self.ventana(a[1])?;
                self.dibujar_lista(ex, a[1], a[0], palabra(d[0]), false);
                Ok(0)
            }
            REFRESHGADGETS => {
                self.ventana(a[1])?;
                self.dibujar_lista(ex, a[1], a[0], -1, false);
                Ok(0)
            }
            ONGADGET | OFFGADGET => hecho(self.on_off_gadget(ex, a[0], a[1], f == ONGADGET)),
            ITEMADDRESS => self.item_address(ex, a[0], d[0]),
            ONMENU | OFFMENU => hecho(self.on_off_menu(ex, a[0], d[0], f == ONMENU)),
            // Una pantalla propia es, de momento, el Workbench (cada ventana ya es una del escritorio): se cuenta
            // como un bloqueo, y CloseScreen lo suelta. Provisional (ver la bitácora).
            OPENSCREEN | OPENSCREENTAGLIST => self.lock_pub_screen(ex, 0),
            CLOSESCREEN => {
                if a[0] != self.workbench() {
                    return Err(Fallo::PantallaDesconocida { dir: a[0] });
                }
                self.unlock_pub_screen(ex, 0, a[0]).map(|_| 1)
            }
            GETDEFAULTPUBSCREEN => {
                if a[0] != 0 {
                    ex.escribir_bytes(a[0], b"Workbench\0")?;
                }
                Ok(0)
            }
            INTUITEXTLENGTH => {
                let t = ex.l32(a[0].wrapping_add(12))?;
                Ok(ex.cadena(t)?.len() as u32 * crate::fuente::ANCHO)
            }
            _ => Err(Fallo::NoImplementada { n: self.numeros.intuition + f as u16 }),
        }
    }
}
