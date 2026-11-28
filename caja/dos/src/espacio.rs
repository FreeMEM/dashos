//! El espacio de nombres de la caja (S4b): volúmenes, asignaciones y cómo se resuelve una ruta del Amiga.
//!
//! - Un volumen es una raíz del servicio de archivos con un nombre (`SYS`, `Usuarios`, `RAM`...).
//! - Una asignación es un nombre que lleva a uno o más cajones (`AssignLock`, `AssignAdd`: multiasignación, se busca
//!   en orden), a una ruta que se fija la primera vez que se usa (`AssignLate`) o a una ruta que se resuelve cada vez
//!   (`AssignPath`). Una que lleva a otra que lleva a la primera es un bucle: a los `MAX_NIVELES` niveles,
//!   `ERROR_TOO_MANY_LEVELS`.
//! - `PROGDIR:` es el cajón del programa (`pr_HomeDir`).
//!
//! Todo esto se refleja en `DosList` en la memoria de la caja, para `LockDosList`, `NextDosEntry`, `FindDosEntry` e
//! `id_VolumeNode`. Lo que manda es la tabla de dos: los programas leen esas `DosList`, pero no se miran al resolver.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::errores::*;
use crate::estructuras::*;
use crate::fs::{Asa, Datos, ErrorFs, LARGO_CAMINO, SistemaArchivos};
use crate::nombres::{self, Paso, clave_latin1, partir};
use crate::*;

/// Las capas de las asignaciones del sistema de S4b para una sesión.
#[derive(Clone, Debug, Default)]
pub struct CapasS4b<'a> {
    /// El usuario: `Home:` es `Usuarios:<usuario>`.
    pub usuario: &'a str,
    /// Los cajones de `Paquetes:` de los paquetes instalados, en orden.
    pub paquetes: &'a [&'a str],
    /// Perfil "aplicación" o "suelto" de S6b: sin la capa del usuario en `C:`, `LIBS:` y `FONTS:`.
    pub encerrado: bool,
}

impl<F: SistemaArchivos> Dos<F> {
    /// Para ramlib (la carga de bibliotecas y dispositivos de disco, en la caja): lee el fichero `ruta` (con las
    /// asignaciones y `PROGDIR:` de la tarea en marcha) y lo carga con `c.cargar_segmentos`, que puede reservar en
    /// otra memoria que la de la tarea. La lista de segmentos no queda apuntada como de la tarea (no es un `LoadSeg`
    /// suyo): la suelta quien la cargó, con `c.descargar_segmentos`. El error, el de dos.
    pub fn cargar_de_disco<C: Caja>(&mut self, c: &mut C, ruta: &[u8]) -> Result<u32, u32> {
        let datos = self.leer_entero(c, ruta, 16 << 20).map_err(|e| match e {
            E::Io(n) => n,
            E::Mal(f) => f.codigo(),
        })?;
        let seg = c.cargar_segmentos(&datos)?;
        if seg == 0 {
            return Err(ERROR_BAD_HUNK);
        }
        Ok(seg)
    }

    /// Lee un fichero entero (hasta `max` bytes) por su ruta del Amiga, para la caja (sus ajustes).
    pub fn leer_fichero<C: Caja>(&mut self, c: &mut C, ruta: &[u8], max: u64) -> Result<Vec<u8>, u32> {
        self.leer_entero(c, ruta, max).map_err(|e| match e {
            E::Io(n) => n,
            E::Mal(f) => f.codigo(),
        })
    }

    // --- Volúmenes ---

    /// Monta una raíz del servicio como volumen `nombre` (sin `:`). La raíz es de dos desde ahora. El primero que se
    /// monte es el de arranque (el del cerrojo 0), salvo que se elija otro con `poner_arranque`.
    pub fn montar<C: Caja>(&mut self, c: &mut C, nombre: &str, raiz: Asa, solo_lectura: bool) -> Result<(), u32> {
        let n = nombres::a_latin1(nombre).ok_or(ERROR_INVALID_COMPONENT_NAME)?;
        if n.is_empty() || n.len() > 30 || n.contains(&b':') || n.contains(&b'/') || n.contains(&0) {
            return Err(ERROR_INVALID_COMPONENT_NAME);
        }
        let k = clave_latin1(&n);
        if self.nombres.contains_key(&k) || k == b"PROGDIR" {
            return Err(ERROR_OBJECT_EXISTS);
        }
        let r = (|| -> R<(u32, u32)> {
            self.asegurar_cabeza(c)?;
            let puerto = self.reservar_fijo(c, 34)?;
            Self::e8(c, puerto + 8, 4)?; // NT_MSGPORT
            let dl = self.reservar_fijo(c, doslist::TAMANO)?;
            let bstr = self.bstr_fijo(c, &n)?;
            Self::e32(c, dl + doslist::DOL_TYPE, doslist::DLT_VOLUME)?;
            Self::e32(c, dl + doslist::DOL_TASK, puerto)?;
            Self::e32(c, dl + doslist::DOL_DISKTYPE, infodata::ID_INTER_FFS_DISK)?;
            Self::e32(c, dl + doslist::DOL_NAME, bstr >> 2)?;
            let (s, us) = c.hora();
            Self::poner_fecha(c, dl + doslist::DOL_VOLUMEDATE, s, us)?;
            Ok((dl, puerto))
        })();
        let (dl, puerto) = r.map_err(|e| match e {
            E::Io(x) => x,
            E::Mal(f) => f.codigo(),
        })?;
        let i = self.volumenes.len();
        // Decisión 8 del usuario (2026-11-13): el volumen de solo lectura lo controla dos, volumen a volumen: la guardia
        // (`guardia.rs`) niega lo que cambie el disco desde su raíz con ERROR_DISK_WRITE_PROTECTED.
        if solo_lectura {
            self.fs.marcar_solo_lectura(raiz);
        }
        self.volumenes.push(Volumen { nombre: n, raiz, solo_lectura, doslist: dl, puerto });
        self.nombres.insert(k, Nombre::Volumen(i));
        if self.arranque.is_none() {
            self.arranque = Some(i);
        }
        let _ = self.enlazar(c);
        Ok(())
    }

    /// Elige el volumen de arranque (el del cerrojo 0, el de `:` sin cajón actual). `false` si no está montado.
    pub fn poner_arranque(&mut self, nombre: &str) -> bool {
        let Some(n) = nombres::a_latin1(nombre) else { return false };
        match self.nombres.get(&clave_latin1(&n)) {
            Some(Nombre::Volumen(i)) => {
                self.arranque = Some(*i);
                true
            }
            _ => false,
        }
    }

    // --- Asignaciones desde la caja (las mismas funciones que llama el 68k) ---

    /// `Assign nombre: ruta` (como `AssignLock` con un cerrojo de la ruta). Cambia la que hubiera.
    pub fn asignar<C: Caja>(&mut self, c: &mut C, nombre: &str, ruta: &str) -> Result<(), u32> {
        self.asignar_con(c, nombre, ruta, false)
    }

    /// `Assign nombre: ruta ADD`: un cajón más al final de una multiasignación (la hace si no existe).
    pub fn asignar_mas<C: Caja>(&mut self, c: &mut C, nombre: &str, ruta: &str) -> Result<(), u32> {
        self.asignar_con(c, nombre, ruta, true)
    }

    fn asignar_con<C: Caja>(&mut self, c: &mut C, nombre: &str, ruta: &str, anadir: bool) -> Result<(), u32> {
        let n = nombres::a_latin1(nombre).ok_or(ERROR_INVALID_COMPONENT_NAME)?;
        let r = nombres::a_latin1(ruta).ok_or(ERROR_INVALID_COMPONENT_NAME)?;
        let res = (|| -> R<()> {
            let l = self.lock_ruta(c, &r, SHARED_LOCK)?;
            let existe = matches!(
                self.nombres.get(&clave_latin1(&n)),
                Some(Nombre::Asignacion(Asignacion { destino: Destino::Cerrojos(_), .. }))
            );
            let r = if anadir && existe { self.assign_add(c, &n, l) } else { self.assign_lock(c, &n, l) };
            if r.is_err() {
                self.soltar_cerrojo(c, l);
            }
            r
        })();
        res.map_err(|e| match e {
            E::Io(x) => x,
            E::Mal(f) => f.codigo(),
        })
    }

    /// `Assign nombre: ruta DEFER`.
    pub fn asignar_tarde<C: Caja>(&mut self, c: &mut C, nombre: &str, ruta: &str) -> Result<(), u32> {
        let n = nombres::a_latin1(nombre).ok_or(ERROR_INVALID_COMPONENT_NAME)?;
        let r = nombres::a_latin1(ruta).ok_or(ERROR_INVALID_COMPONENT_NAME)?;
        self.assign_texto(c, &n, Destino::Tarde(r)).map_err(|e| match e {
            E::Io(x) => x,
            E::Mal(f) => f.codigo(),
        })
    }

    /// `Assign nombre: ruta`, creando antes el cajón si no existe (como el `T:` de `RAM:` de un Amiga).
    pub fn asignar_creando<C: Caja>(&mut self, c: &mut C, nombre: &str, ruta: &str) -> Result<(), u32> {
        if self.asignar(c, nombre, ruta).is_ok() {
            return Ok(());
        }
        // Un nombre que no vale no crea nada.
        let n = nombres::a_latin1(nombre).ok_or(ERROR_INVALID_COMPONENT_NAME)?;
        Self::nombre_de_asignacion(&n).map_err(|e| match e {
            E::Io(x) => x,
            E::Mal(f) => f.codigo(),
        })?;
        let r = nombres::a_latin1(ruta).ok_or(ERROR_INVALID_COMPONENT_NAME)?;
        let creado = self.crear_cajon(c, &r).map(|l| self.soltar_cerrojo(c, l));
        creado.map_err(|e| match e {
            E::Io(x) => x,
            E::Mal(f) => f.codigo(),
        })?;
        self.asignar(c, nombre, ruta)
    }

    /// `Assign nombre: ruta PATH`.
    pub fn asignar_ruta<C: Caja>(&mut self, c: &mut C, nombre: &str, ruta: &str) -> Result<(), u32> {
        let n = nombres::a_latin1(nombre).ok_or(ERROR_INVALID_COMPONENT_NAME)?;
        let r = nombres::a_latin1(ruta).ok_or(ERROR_INVALID_COMPONENT_NAME)?;
        self.assign_texto(c, &n, Destino::Ruta(r)).map_err(|e| match e {
            E::Io(x) => x,
            E::Mal(f) => f.codigo(),
        })
    }

    /// Las asignaciones del sistema de S4b, con sus capas (usuario, paquetes, sistema). Una capa cuyo cajón no
    /// existe se salta; `ENV:` y `T:` se crean en `RAM:` si faltan. Devuelve las que no se pudieron hacer, con su
    /// error.
    pub fn asignaciones_s4b<C: Caja>(&mut self, c: &mut C, capas: &CapasS4b) -> Vec<(String, u32)> {
        let mut fallos = Vec::new();
        let home = alloc::format!("Usuarios:{}", capas.usuario);
        if let Err(e) = self.asignar(c, "Home", &home) {
            fallos.push((String::from("Home"), e));
        }
        // (asignación, cajón en Home:, cajón en cada paquete, cajón en SYS:)
        let tabla: [(&str, Option<&str>, Option<&str>, &str); 7] = [
            ("C", if capas.encerrado { None } else { Some("C") }, Some("C"), "C"),
            ("LIBS", if capas.encerrado { None } else { Some("Libs") }, Some("Libs"), "Libs"),
            ("DEVS", None, Some("Devs"), "Devs"),
            ("FONTS", if capas.encerrado { None } else { Some("Fonts") }, Some("Fonts"), "Fonts"),
            ("LOCALE", None, Some("Locale"), "Locale"),
            ("ENVARC", Some("Prefs/Env-Archive"), None, "Prefs/Env-Archive"),
            ("S", Some("S"), None, "S"),
        ];
        for (nombre, en_home, en_paquete, en_sys) in tabla {
            let mut rutas = Vec::new();
            if let Some(h) = en_home {
                rutas.push(alloc::format!("Home:{h}"));
            }
            if let Some(p) = en_paquete {
                for paq in capas.paquetes {
                    rutas.push(alloc::format!("Paquetes:{paq}/{p}"));
                }
            }
            rutas.push(alloc::format!("SYS:{en_sys}"));
            let mut hecha = false;
            let mut primer_error = None;
            for r in &rutas {
                let res = if hecha { self.asignar_mas(c, nombre, r) } else { self.asignar(c, nombre, r) };
                match res {
                    Ok(()) => hecha = true,
                    Err(e) => {
                        primer_error.get_or_insert(e);
                    }
                }
            }
            if !hecha {
                fallos.push((String::from(nombre), primer_error.unwrap_or(ERROR_OBJECT_NOT_FOUND)));
            }
        }
        for (nombre, cajon) in [("ENV", "Env"), ("T", "T")] {
            let ruta = alloc::format!("RAM:{cajon}");
            if self.asignar(c, nombre, &ruta).is_err() {
                // Crear el cajón en RAM: y probar otra vez.
                let r = nombres::a_latin1(&ruta).unwrap_or_default();
                let creado = self.crear_cajon(c, &r).map(|l| self.soltar_cerrojo(c, l));
                let res = creado.map_err(|e| match e {
                    E::Io(x) => x,
                    E::Mal(f) => f.codigo(),
                });
                if let Err(e) = res.and_then(|_| self.asignar(c, nombre, &ruta)) {
                    fallos.push((String::from(nombre), e));
                }
            }
        }
        fallos
    }

    // --- Las funciones de asignación (AssignLock, AssignLate, AssignPath, AssignAdd, RemAssignList) ---

    /// Comprueba un nombre de asignación (sin `:`, o con uno al final) y devuelve el nombre limpio.
    fn nombre_de_asignacion(n: &[u8]) -> R<Vec<u8>> {
        let n = n.strip_suffix(b":").unwrap_or(n);
        if n.is_empty() || n.len() > 255 || n.contains(&b':') || n.contains(&b'/') {
            return Err(E::Io(ERROR_INVALID_COMPONENT_NAME));
        }
        Ok(n.to_vec())
    }

    /// Mira que el nombre se pueda usar para una asignación: no es un volumen ni `PROGDIR`.
    fn libre_para_asignar(&self, k: &[u8]) -> R<()> {
        if k == b"PROGDIR" || matches!(self.nombres.get(k), Some(Nombre::Volumen(_))) {
            return Err(E::Io(ERROR_OBJECT_EXISTS));
        }
        Ok(())
    }

    /// Un cerrojo que pasa a ser de una asignación: tiene que ser de dos, de un cajón y no de otra asignación.
    fn tomar_para_asignar<C: Caja>(&mut self, c: &C, l: u32) -> R<()> {
        let Some(x) = self.cerrojos.get(&l) else {
            return Err(Fallo::CerrojoDesconocido { bptr: l >> 2 }.into());
        };
        if x.de_asignacion {
            return Err(Fallo::CerrojoDeAsignacion { bptr: l >> 2 }.into());
        }
        if !x.cajon {
            return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
        }
        let _ = c;
        Ok(())
    }

    /// AssignLock: `l` es la dirección del `FileLock` (0 quita la asignación). Si va bien, el cerrojo es de dos.
    pub(crate) fn assign_lock<C: Caja>(&mut self, c: &mut C, nombre: &[u8], l: u32) -> R<()> {
        let n = Self::nombre_de_asignacion(nombre)?;
        let k = clave_latin1(&n);
        self.libre_para_asignar(&k)?;
        if l == 0 {
            self.quitar_asignacion(c, &k);
            return Ok(());
        }
        self.tomar_para_asignar(c, l)?;
        self.poner_asignacion(c, n, Destino::Cerrojos(vec![l]))
    }

    /// AssignAdd: un cajón más al final de una multiasignación que ya existe.
    pub(crate) fn assign_add<C: Caja>(&mut self, c: &mut C, nombre: &[u8], l: u32) -> R<()> {
        let n = Self::nombre_de_asignacion(nombre)?;
        let k = clave_latin1(&n);
        self.libre_para_asignar(&k)?;
        match self.nombres.get(&k) {
            Some(Nombre::Asignacion(Asignacion { destino: Destino::Cerrojos(_), .. })) => {}
            Some(_) => return Err(E::Io(ERROR_OBJECT_WRONG_TYPE)),
            None => return Err(E::Io(ERROR_OBJECT_NOT_FOUND)),
        }
        self.tomar_para_asignar(c, l)?;
        if let Some(Nombre::Asignacion(a)) = self.nombres.get_mut(&k)
            && let Destino::Cerrojos(v) = &mut a.destino
        {
            v.push(l);
        }
        if let Some(x) = self.cerrojos.get_mut(&l) {
            x.de_asignacion = true;
            x.dueno = 0;
        }
        self.reflejar_asignacion(c, &k)?;
        Ok(())
    }

    /// AssignLate y AssignPath.
    pub(crate) fn assign_texto<C: Caja>(&mut self, c: &mut C, nombre: &[u8], d: Destino) -> R<()> {
        let n = Self::nombre_de_asignacion(nombre)?;
        let k = clave_latin1(&n);
        self.libre_para_asignar(&k)?;
        if let Destino::Tarde(r) | Destino::Ruta(r) = &d
            && (r.is_empty() || r.len() > 1024)
        {
            return Err(E::Io(ERROR_OBJECT_NOT_FOUND));
        }
        self.poner_asignacion(c, n, d)
    }

    /// RemAssignList: quita de una multiasignación el cajón que sea el mismo que `l` (que no se suelta). `true` si
    /// estaba.
    pub(crate) fn rem_assign_list<C: Caja>(&mut self, c: &mut C, nombre: &[u8], l: u32) -> R<bool> {
        let n = Self::nombre_de_asignacion(nombre)?;
        let k = clave_latin1(&n);
        let s = match self.cerrojos.get(&l) {
            Some(x) => x.sitio.clave(),
            None => return Err(Fallo::CerrojoDesconocido { bptr: l >> 2 }.into()),
        };
        let quitado = match self.nombres.get_mut(&k) {
            Some(Nombre::Asignacion(Asignacion { destino: Destino::Cerrojos(v), .. })) => {
                let cerrojos = &self.cerrojos;
                let pos = v.iter().position(|d| cerrojos.get(d).is_some_and(|x| x.sitio.clave() == s));
                pos.map(|i| v.remove(i))
            }
            _ => None,
        };
        let Some(q) = quitado else { return Ok(false) };
        if let Some(x) = self.cerrojos.get_mut(&q) {
            x.de_asignacion = false;
        }
        self.soltar_cerrojo(c, q);
        let vacia = matches!(
            self.nombres.get(&k),
            Some(Nombre::Asignacion(Asignacion { destino: Destino::Cerrojos(v), .. })) if v.is_empty()
        );
        if vacia {
            self.quitar_asignacion(c, &k);
        } else {
            self.reflejar_asignacion(c, &k)?;
        }
        Ok(true)
    }

    /// Pone (o cambia) una asignación y su `DosList`.
    fn poner_asignacion<C: Caja>(&mut self, c: &mut C, n: Vec<u8>, d: Destino) -> R<()> {
        let k = clave_latin1(&n);
        self.asegurar_cabeza(c)?;
        // La DosList nueva antes de soltar la vieja: si no hay memoria, se queda como estaba.
        let dl = self.reservar_fijo(c, doslist::TAMANO)?;
        let bstr = match self.bstr_fijo(c, &n) {
            Ok(b) => b,
            Err(e) => {
                self.soltar_fijo(c, dl);
                return Err(e);
            }
        };
        self.quitar_asignacion(c, &k);
        if let Destino::Cerrojos(v) = &d {
            for l in v {
                if let Some(x) = self.cerrojos.get_mut(l) {
                    x.de_asignacion = true;
                    x.dueno = 0;
                }
            }
        }
        Self::e32(c, dl + doslist::DOL_NAME, bstr >> 2)?;
        self.nombres.insert(
            k.clone(),
            Nombre::Asignacion(Asignacion {
                nombre: n,
                destino: d,
                doslist: dl,
                bstr,
                listas: Vec::new(),
                cadena: (0, 0),
            }),
        );
        self.reflejar_asignacion(c, &k)?;
        self.enlazar(c)?;
        Ok(())
    }

    /// Quita una asignación (si la hay), suelta sus cerrojos y su `DosList`.
    pub(crate) fn quitar_asignacion<C: Caja>(&mut self, c: &mut C, k: &[u8]) {
        let Some(Nombre::Asignacion(a)) = self.nombres.get(k).cloned() else { return };
        self.nombres.remove(k);
        if let Destino::Cerrojos(v) = &a.destino {
            for l in v {
                if let Some(x) = self.cerrojos.get_mut(l) {
                    x.de_asignacion = false;
                }
                self.soltar_cerrojo(c, *l);
            }
        }
        for al in &a.listas {
            self.soltar_fijo(c, *al);
        }
        if a.cadena.0 != 0 {
            self.soltar_fijo(c, a.cadena.0);
        }
        self.soltar_fijo(c, a.doslist);
        self.soltar_fijo(c, a.bstr);
        let _ = self.enlazar(c);
    }

    /// Escribe en la `DosList` de una asignación su tipo y su destino.
    fn reflejar_asignacion<C: Caja>(&mut self, c: &mut C, k: &[u8]) -> R<()> {
        let Some(Nombre::Asignacion(a)) = self.nombres.get(k).cloned() else { return Ok(()) };
        for al in &a.listas {
            self.soltar_fijo(c, *al);
        }
        if a.cadena.0 != 0 {
            self.soltar_fijo(c, a.cadena.0);
        }
        let dl = a.doslist;
        let mut listas = Vec::new();
        let mut cadena = (0, 0);
        Self::cero(c, dl + doslist::DOL_TASK, 36 - 8)?;
        match &a.destino {
            Destino::Cerrojos(v) => {
                Self::e32(c, dl + doslist::DOL_TYPE, doslist::DLT_DIRECTORY)?;
                if let Some(&primero) = v.first() {
                    Self::e32(c, dl + doslist::DOL_LOCK, primero >> 2)?;
                    let vol = self.cerrojos.get(&primero).map(|x| x.sitio.vol);
                    let puerto = vol.and_then(|i| self.volumenes.get(i)).map_or(0, |v| v.puerto);
                    Self::e32(c, dl + doslist::DOL_TASK, puerto)?;
                }
                // Los de más, en una lista de AssignList.
                let mut anterior = dl + doslist::DOL_LIST;
                for &l in v.iter().skip(1) {
                    let al = self.reservar_fijo(c, assignlist::TAMANO)?;
                    listas.push(al);
                    Self::e32(c, al + assignlist::AL_LOCK, l >> 2)?;
                    Self::e32(c, anterior, al)?;
                    anterior = al + assignlist::AL_NEXT;
                }
            }
            Destino::Tarde(r) | Destino::Ruta(r) => {
                let tipo =
                    if matches!(a.destino, Destino::Tarde(_)) { doslist::DLT_LATE } else { doslist::DLT_NONBINDING };
                Self::e32(c, dl + doslist::DOL_TYPE, tipo)?;
                let largo = r.len() as u32 + 1;
                let s = self.reservar_fijo(c, largo)?;
                Self::poner(c, s, r)?;
                cadena = (s, largo);
                Self::e32(c, dl + doslist::DOL_ASSIGNNAME, s)?;
            }
        }
        if let Some(Nombre::Asignacion(a)) = self.nombres.get_mut(k) {
            a.listas = listas;
            a.cadena = cadena;
        }
        Ok(())
    }

    pub(crate) fn asegurar_cabeza<C: Caja>(&mut self, c: &mut C) -> R<()> {
        if self.cabeza_doslist == 0 {
            self.cabeza_doslist = self.reservar_fijo(c, doslist::TAMANO)?;
        }
        Ok(())
    }

    /// Las `DosList` en orden: volúmenes (por orden de montaje) y asignaciones (por nombre).
    pub(crate) fn orden_doslist(&self) -> Vec<(u32, u32, Vec<u8>)> {
        let mut v: Vec<(u32, u32, Vec<u8>)> =
            self.volumenes.iter().map(|x| (x.doslist, doslist::DLT_VOLUME, x.nombre.clone())).collect();
        for n in self.nombres.values() {
            if let Nombre::Asignacion(a) = n {
                let t = match a.destino {
                    Destino::Cerrojos(_) => doslist::DLT_DIRECTORY,
                    Destino::Tarde(_) => doslist::DLT_LATE,
                    Destino::Ruta(_) => doslist::DLT_NONBINDING,
                };
                v.push((a.doslist, t, a.nombre.clone()));
            }
        }
        v
    }

    /// Encadena las `DosList` por `dol_Next`, desde la cabeza.
    fn enlazar<C: Caja>(&mut self, c: &mut C) -> R<()> {
        let orden = self.orden_doslist();
        let mut anterior = self.cabeza_doslist;
        for (d, _, _) in &orden {
            Self::e32(c, anterior + doslist::DOL_NEXT, d >> 2)?;
            anterior = *d;
        }
        if anterior != 0 {
            Self::e32(c, anterior + doslist::DOL_NEXT, 0)?;
        }
        Ok(())
    }

    // --- Resolver una ruta ---

    /// El sitio de un cerrojo (0: la raíz del volumen de arranque).
    pub(crate) fn sitio_de_cerrojo(&self, bptr: u32) -> R<Sitio> {
        if bptr == 0 {
            let vol = self.arranque.ok_or(E::Io(ERROR_DEVICE_NOT_MOUNTED))?;
            return Ok(Sitio { vol, camino: Vec::new() });
        }
        let d = Self::baddr(bptr)?;
        self.cerrojos.get(&d).map(|x| x.sitio.clone()).ok_or(Fallo::CerrojoDesconocido { bptr }.into())
    }

    /// Los sitios a los que lleva una ruta, en orden (más de uno con una multiasignación). No mira si existen.
    pub(crate) fn sitios<C: Caja>(&mut self, c: &mut C, ruta: &[u8]) -> R<Vec<Sitio>> {
        self.sitios_n(c, ruta, 0)
    }

    fn sitios_n<C: Caja>(&mut self, c: &mut C, ruta: &[u8], prof: u32) -> R<Vec<Sitio>> {
        if prof > MAX_NIVELES {
            return Err(E::Io(ERROR_TOO_MANY_LEVELS));
        }
        let r = partir(ruta).map_err(|_| E::Io(ERROR_INVALID_COMPONENT_NAME))?;
        let bases = match &r.dispositivo {
            None => vec![self.sitio_de_cerrojo(self.campo(c, process::PR_CURRENTDIR))?],
            Some(d) if d.is_empty() => {
                let s = self.sitio_de_cerrojo(self.campo(c, process::PR_CURRENTDIR))?;
                vec![Sitio { vol: s.vol, camino: Vec::new() }]
            }
            Some(d) => self.bases_de(c, d, prof)?,
        };
        let mut res = Vec::new();
        let mut primer_error = None;
        for mut b in bases {
            let mut bien = true;
            for p in &r.pasos {
                match p {
                    Paso::Subir => {
                        if b.camino.pop().is_none() {
                            bien = false;
                            break;
                        }
                    }
                    Paso::Nombre(n) => b.camino.push(n.clone()),
                }
            }
            if bien {
                res.push(b);
            } else {
                primer_error.get_or_insert(E::Io(ERROR_OBJECT_NOT_FOUND));
            }
        }
        if res.is_empty() { Err(primer_error.unwrap_or(E::Io(ERROR_OBJECT_NOT_FOUND))) } else { Ok(res) }
    }

    /// Los sitios de un nombre de dispositivo (sin `:`).
    fn bases_de<C: Caja>(&mut self, c: &mut C, nombre: &[u8], prof: u32) -> R<Vec<Sitio>> {
        let k = clave_latin1(nombre);
        if k == b"PROGDIR" {
            let h = self.campo(c, process::PR_HOMEDIR);
            if h == 0 {
                return Err(E::Io(ERROR_DEVICE_NOT_MOUNTED));
            }
            return Ok(vec![self.sitio_de_cerrojo(h)?]);
        }
        let destino = match self.nombres.get(&k) {
            None => return Err(E::Io(ERROR_DEVICE_NOT_MOUNTED)),
            Some(Nombre::Volumen(i)) => return Ok(vec![Sitio { vol: *i, camino: Vec::new() }]),
            Some(Nombre::Asignacion(a)) => a.destino.clone(),
        };
        match destino {
            Destino::Cerrojos(v) => {
                Ok(v.iter().filter_map(|l| self.cerrojos.get(l)).map(|x| x.sitio.clone()).collect())
            }
            Destino::Ruta(r) => self.sitios_n(c, &r, prof + 1),
            Destino::Tarde(r) => {
                // La primera vez que se usa, se convierte en un cerrojo del primer sitio que exista.
                let sitios = self.sitios_n(c, &r, prof + 1)?;
                let (sitio, asa, datos) = self.abrir_primero(&sitios)?;
                if datos.tipo != fs::Tipo::Cajon {
                    let _ = self.fs.cerrar(asa);
                    return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
                }
                let l = self.nuevo_cerrojo(c, sitio.clone(), asa, false, true)?;
                if let Some(Nombre::Asignacion(a)) = self.nombres.get_mut(&k) {
                    a.destino = Destino::Cerrojos(vec![l]);
                }
                if let Some(x) = self.cerrojos.get_mut(&l) {
                    x.de_asignacion = true;
                    x.dueno = 0;
                }
                self.reflejar_asignacion(c, &k)?;
                self.enlazar(c)?;
                Ok(vec![sitio])
            }
        }
    }

    // --- Hablar con el servicio por sitios ---

    /// Abre un camino desde una asa, en trozos de hasta `LARGO_CAMINO` bytes (lo que cabe en un mensaje).
    pub(crate) fn buscar_por_trozos(&mut self, desde: Asa, partes: &[String]) -> Result<(Asa, Datos), ErrorFs> {
        let mut actual: Option<Asa> = None;
        let mut i = 0;
        loop {
            let mut j = i;
            let mut largo = 0;
            while j < partes.len() {
                let l = partes[j].len() + usize::from(j > i);
                if largo + l > LARGO_CAMINO {
                    break;
                }
                largo += l;
                j += 1;
            }
            if j == i && i < partes.len() {
                if let Some(a) = actual {
                    let _ = self.fs.cerrar(a);
                }
                return Err(ErrorFs::MalFormado);
            }
            let camino = partes[i..j].join("/");
            let r = self.fs.buscar(actual.unwrap_or(desde), &camino);
            if let Some(a) = actual {
                let _ = self.fs.cerrar(a);
            }
            let (asa, datos) = r?;
            if j >= partes.len() {
                return Ok((asa, datos));
            }
            actual = Some(asa);
            i = j;
        }
    }

    pub(crate) fn abrir_sitio(&mut self, s: &Sitio) -> Result<(Asa, Datos), ErrorFs> {
        let raiz = self.volumenes.get(s.vol).ok_or(ErrorFs::AsaMala)?.raiz;
        self.buscar_por_trozos(raiz, &s.camino)
    }

    /// Los datos de un sitio sin dejar nada abierto.
    pub(crate) fn mirar_sitio(&mut self, s: &Sitio) -> Result<Datos, ErrorFs> {
        let raiz = self.volumenes.get(s.vol).ok_or(ErrorFs::AsaMala)?.raiz;
        let total: usize = s.camino.iter().map(|p| p.len() + 1).sum();
        if total <= LARGO_CAMINO + 1 {
            return self.fs.mirar(raiz, &s.camino.join("/"));
        }
        let (asa, d) = self.buscar_por_trozos(raiz, &s.camino)?;
        let _ = self.fs.cerrar(asa);
        Ok(d)
    }

    /// Abre el cajón de un sitio y devuelve su asa y el último nombre. La raíz no tiene cajón.
    pub(crate) fn abrir_padre(&mut self, s: &Sitio) -> R<(Asa, String)> {
        let Some((ultimo, padre)) = s.camino.split_last() else {
            return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
        };
        let raiz = self.volumenes.get(s.vol).ok_or(E::Io(ERROR_INVALID_LOCK))?.raiz;
        let (asa, d) = self.buscar_por_trozos(raiz, padre).map_err(|e| de_fs(e, Op::Otra))?;
        if d.tipo != fs::Tipo::Cajon {
            let _ = self.fs.cerrar(asa);
            return Err(E::Io(ERROR_OBJECT_WRONG_TYPE));
        }
        Ok((asa, ultimo.clone()))
    }

    /// Abre el primero de los sitios que exista (lo que hace una multiasignación al buscar).
    pub(crate) fn abrir_primero(&mut self, sitios: &[Sitio]) -> R<(Sitio, Asa, Datos)> {
        let mut primer_error = None;
        for s in sitios {
            match self.abrir_sitio(s) {
                Ok((a, d)) => return Ok((s.clone(), a, d)),
                Err(ErrorFs::NoExiste) => {
                    primer_error.get_or_insert(E::Io(ERROR_OBJECT_NOT_FOUND));
                }
                Err(e) => return Err(de_fs(e, Op::Leer)),
            }
        }
        Err(primer_error.unwrap_or(E::Io(ERROR_OBJECT_NOT_FOUND)))
    }

    /// El primero de los sitios que exista, sin dejarlo abierto.
    pub(crate) fn primero_que_existe(&mut self, sitios: &[Sitio]) -> R<(Sitio, Datos)> {
        let mut primer_error = None;
        for s in sitios {
            match self.mirar_sitio(s) {
                Ok(d) => return Ok((s.clone(), d)),
                Err(ErrorFs::NoExiste) => {
                    primer_error.get_or_insert(E::Io(ERROR_OBJECT_NOT_FOUND));
                }
                Err(e) => return Err(de_fs(e, Op::Otra)),
            }
        }
        Err(primer_error.unwrap_or(E::Io(ERROR_OBJECT_NOT_FOUND)))
    }

    /// Cuántos cerrojos y ficheros compartidos tiene un sitio, y si alguno es exclusivo.
    pub(crate) fn uso(&self, s: &Sitio) -> (usize, bool) {
        let k = s.clave();
        let mut n = 0;
        let mut excl = false;
        for x in self.cerrojos.values() {
            if x.sitio.vol == s.vol && x.sitio.clave() == k {
                n += 1;
                excl |= x.exclusivo;
            }
        }
        for f in self.ficheros.values() {
            if let Flujo::Disco { sitio, exclusivo, .. } = &f.flujo
                && sitio.vol == s.vol
                && sitio.clave() == k
            {
                n += 1;
                excl |= *exclusivo;
            }
        }
        (n, excl)
    }

    /// Tras un `Rename`, los cerrojos y ficheros que había dentro de lo movido siguen a su sitio nuevo.
    pub(crate) fn mover_sitios(&mut self, viejo: &Sitio, nuevo: &Sitio) {
        let kv = viejo.clave().1;
        let mover = |s: &mut Sitio| {
            if s.vol == viejo.vol && s.camino.len() >= kv.len() {
                let pref: Vec<String> = s.camino[..kv.len()].iter().map(|p| nombres::clave(p)).collect();
                if pref == kv {
                    let mut c = nuevo.camino.clone();
                    c.extend_from_slice(&s.camino[kv.len()..]);
                    s.camino = c;
                }
            }
        };
        for x in self.cerrojos.values_mut() {
            mover(&mut x.sitio);
        }
        for f in self.ficheros.values_mut() {
            if let Flujo::Disco { sitio, .. } = &mut f.flujo {
                mover(sitio);
            }
        }
    }

    // --- NextDosEntry, FindDosEntry ---

    fn tipo_en_marcas(tipo: u32, marcas: u32) -> bool {
        match tipo {
            doslist::DLT_VOLUME => marcas & ldf::VOLUMES != 0,
            doslist::DLT_DEVICE => marcas & ldf::DEVICES != 0,
            _ => marcas & ldf::ASSIGNS != 0,
        }
    }

    /// La siguiente `DosList` tras `dl` (o tras la cabeza de `LockDosList`) que sea de los tipos de `marcas`.
    pub(crate) fn siguiente_doslist(&self, dl: u32, marcas: u32, nombre: Option<&[u8]>) -> Option<u32> {
        let orden = self.orden_doslist();
        let desde = if dl == self.cabeza_doslist { 0 } else { orden.iter().position(|(d, _, _)| *d == dl)? + 1 };
        let k = nombre.map(|n| clave_latin1(n.strip_suffix(b":").unwrap_or(n)));
        orden[desde..]
            .iter()
            .find(|(_, t, n)| Self::tipo_en_marcas(*t, marcas) && k.as_ref().is_none_or(|k| clave_latin1(n) == *k))
            .map(|(d, _, _)| *d)
    }

    // --- GetDeviceProc y FreeDeviceProc ---

    /// GetDeviceProc(name, dp): el "manejador" de lo que nombra `name`. En la caja no hay procesos manejadores (dos
    /// lleva los volúmenes): `dvp_Port` es el puerto de mentira del volumen (el de su `DosList`, que no contesta
    /// paquetes) y `dvp_DevNode` su `DosList`. `dvp_Lock`: con una asignación, un cerrojo de su cajón (lo suelta
    /// FreeDeviceProc: `DVPF_UNLOCK` y `DVPF_ASSIGN`); con un nombre sin `:`, el cajón actual (no se suelta); con un
    /// volumen, 0. Con `dp` de una asignación múltiple, el cajón siguiente (y `dp` se suelta); al acabar, 0 y
    /// ERROR_NO_MORE_ENTRIES. Así funcionan los programas que solo miran a qué volumen va un nombre (Delete de
    /// AmigaOS 3.9); los que manden paquetes al puerto no tendrán respuesta.
    pub(crate) fn get_device_proc<C: Caja>(&mut self, c: &mut C, nombre: u32, dp: u32) -> R<u32> {
        const DVPF_UNLOCK: u32 = 1;
        const DVPF_ASSIGN: u32 = 2;
        let n = Self::cadena(c, nombre)?;
        let siguiente = match self.devprocs.get(&dp) {
            Some(&(_, Some(k))) => k + 1,
            Some(&(_, None)) => {
                self.free_device_proc(c, dp);
                return Err(E::Io(ERROR_NO_MORE_ENTRIES));
            }
            None => 0,
        };
        if dp != 0 {
            self.free_device_proc(c, dp);
        }
        let (vol, cerrojo, banderas, asignacion) = match n.iter().position(|&b| b == b':') {
            None => {
                let l = self.campo(c, process::PR_CURRENTDIR);
                (self.sitio_de_cerrojo(l)?.vol, l, 0, None)
            }
            Some(0) => (self.sitio_de_cerrojo(self.campo(c, process::PR_CURRENTDIR))?.vol, 0, 0, None),
            Some(i) => match self.nombres.get(&clave_latin1(&n[..i])) {
                Some(Nombre::Volumen(v)) => (*v, 0, 0, None),
                Some(Nombre::Asignacion(_)) => {
                    let sitios = self.sitios(c, &n[..=i])?;
                    let Some(s) = sitios.get(siguiente).cloned() else {
                        return Err(E::Io(if siguiente > 0 { ERROR_NO_MORE_ENTRIES } else { ERROR_OBJECT_NOT_FOUND }));
                    };
                    let (asa, d) = self.abrir_sitio(&s).map_err(|e| de_fs(e, Op::Leer))?;
                    let vol = s.vol;
                    let l = self.nuevo_cerrojo(c, s, asa, false, d.tipo == crate::fs::Tipo::Cajon)? >> 2;
                    (vol, l, DVPF_UNLOCK | DVPF_ASSIGN, Some(siguiente))
                }
                None => return Err(E::Io(ERROR_DEVICE_NOT_MOUNTED)),
            },
        };
        let (puerto, doslist) = self.volumenes.get(vol).map_or((0, 0), |v| (v.puerto, v.doslist));
        let d = match Self::reservar(c, 16) {
            Ok(d) => d,
            Err(e) => {
                if banderas & DVPF_UNLOCK != 0 {
                    let _ = self.unlock(c, cerrojo);
                }
                return Err(e);
            }
        };
        Self::e32(c, d, puerto)?;
        Self::e32(c, d + 4, cerrojo)?;
        Self::e32(c, d + 8, banderas)?;
        Self::e32(c, d + 12, doslist)?;
        self.devprocs.insert(d, (if banderas & DVPF_UNLOCK != 0 { cerrojo } else { 0 }, asignacion));
        Ok(d)
    }

    /// FreeDeviceProc(dp): suelta lo que dio GetDeviceProc (y su cerrojo, si era suyo). Con otra cosa, nada.
    pub(crate) fn free_device_proc<C: Caja>(&mut self, c: &mut C, dp: u32) {
        if let Some((l, _)) = self.devprocs.remove(&dp) {
            if l != 0 {
                let _ = self.unlock(c, l);
            }
            c.liberar(dp, 16);
        }
    }
}
