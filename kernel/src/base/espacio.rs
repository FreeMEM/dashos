//! Espacios de direcciones de los procesos (C11, S3).
//!
//! La mitad baja (TTBR0) es de cada proceso, con su ASID: al cambiar de proceso no hace falta vaciar la TLB. Las
//! tablas son de 4 KB y tres niveles (39 bits). Se escriben en el orden del kernel (little-endian desde C33), como las del arranque.

use crate::cerrojo::CerrojoIrq;
use crate::memoria::{paginas, PAGINA};
use crate::mmu;

/// ASID de 16 bits (TCR.AS = 1). Un mapa con uno ocupado por bit. El 0 es de la tabla vacía y nunca se da. Un ASID
/// solo vuelve al mapa después de vaciar sus entradas de la TLB en todos los núcleos (`Drop`), así que dos espacios
/// vivos nunca comparten ASID y uno nuevo nunca hereda traducciones de otro (auditoría, hallazgo 2).
const ASIDS: usize = 1 << 16;
static ASID_OCUPADO: CerrojoIrq<[u64; ASIDS / 64]> = CerrojoIrq::new({
    let mut m = [0u64; ASIDS / 64];
    m[0] = 1;
    m
});

fn asid_nuevo() -> Option<u64> {
    let mut m = ASID_OCUPADO.tomar();
    let (i, palabra) = m.iter().enumerate().find(|(_, w)| **w != u64::MAX)?;
    let b = palabra.trailing_ones() as usize;
    m[i] |= 1 << b;
    Some((i * 64 + b) as u64)
}

fn asid_devolver(asid: u64) {
    let a = asid as usize;
    ASID_OCUPADO.tomar()[a / 64] &= !(1 << (a % 64));
}

/// Tabla de nivel 1 sin nada: TTBR0 cuando no corre ningún proceso, para que el núcleo no siga apuntando a las
/// tablas de uno muerto ni a la proyección identidad del arranque (auditoría, hallazgo 13).
#[repr(C, align(4096))]
struct TablaVacia([u64; 512]);
static VACIA: TablaVacia = TablaVacia([0; 512]);

const VALIDO: u64 = 0b11; // tabla o página
const AF: u64 = 1 << 10;
const NG: u64 = 1 << 11; // no global: va con el ASID
const SH_INTERNO: u64 = 3 << 8;
const ATTR_NORMAL: u64 = 1 << 2;
const ATTR_DISPOSITIVO: u64 = 0 << 2; // MAIR índice 0: nGnRnE
const ATTR_SIN_CACHE: u64 = 2 << 2; // MAIR índice 2: normal sin caché
const AP_EL0: u64 = 1 << 6; // EL0 puede acceder
const AP_SOLO_LECTURA: u64 = 1 << 7;
const PXN: u64 = 1 << 53;
const UXN: u64 = 1 << 54;
const DIRECCION: u64 = 0x0000_ffff_ffff_f000;

#[derive(Clone, Copy)]
pub enum Permiso {
    /// Código del proceso: se lee y se ejecuta en EL0.
    Codigo,
    /// Datos y pila: se leen y escriben en EL0, no se ejecutan.
    Datos,
    /// Datos que solo se leen.
    SoloLectura,
    /// Registros de un aparato (C20): memoria de dispositivo, sin caché ni reordenación; nunca se ejecuta.
    Dispositivo,
    /// Búferes de DMA (C46): memoria normal sin caché, para que el aparato y el proceso vean lo mismo sin limpiar
    /// cachés (en la Pi 4 el DMA no es coherente con ellas). Se lee y escribe; nunca se ejecuta.
    DatosSinCache,
}

pub struct Espacio {
    l1: u64, // dirección física
    asid: u64,
    /// Páginas de tablas (niveles 2 y 3) y páginas propias del proceso: se devuelven al liberar el espacio (C18).
    tablas: alloc::vec::Vec<u64>,
    propias: alloc::vec::Vec<u64>,
    /// Objetos de memoria proyectados, con la dirección donde empiezan: el espacio los mantiene vivos mientras estén
    /// proyectados (C18, C45).
    proyectados: alloc::vec::Vec<(u64, alloc::sync::Arc<super::memoria::Memoria>)>,
    /// El grupo al que se cobran sus tablas y sus páginas propias, y cuánto lleva cobrado (C42).
    cupo: alloc::sync::Arc<crate::cupo::Cupo>,
    cobrado: u64,
}

impl Drop for Espacio {
    fn drop(&mut self) {
        // Que ningún núcleo conserve traducciones de este ASID; solo entonces se puede volver a dar.
        unsafe { core::arch::asm!("dsb ishst", "tlbi aside1is, {}", "dsb ish", "isb", in(reg) self.asid << 48) };
        asid_devolver(self.asid);
        let mut p = paginas();
        for &t in self.tablas.iter().chain(self.propias.iter()) {
            unsafe { p.liberar(t, 1) };
        }
        unsafe { p.liberar(self.l1, 1) };
        self.cupo.devolver(self.cobrado);
    }
}

/// Lo que puede fallar al proyectar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// No queda memoria para el proceso (fuera de la reserva del kernel) o para sus tablas.
    SinMemoria,
    /// Ya hay algo proyectado en esa dirección. No se pisa nunca (antes se pisaba sin vaciar la TLB).
    Ocupada,
    /// Demasiadas proyecciones de objetos de memoria (`MAX_PROYECCIONES`).
    Limite,
    /// La dirección no es del usuario o cae en los primeros 64 KB, que no se proyectan nunca: un puntero nulo del
    /// kernel no debe encontrar datos del usuario (auditoría, hallazgos bajos).
    Direccion,
}

/// Proyecciones de objetos de memoria por proceso, hasta los cupos de S16 (F).
const MAX_PROYECCIONES: usize = 1024;
/// Inicio de la ventana de periféricos de la Pi 4.
const PERIFERICOS: u64 = 0xFC00_0000;

/// Por debajo de esto no se proyecta nada.
pub const VA_MINIMA: u64 = 0x1_0000;
/// Fin de la mitad baja: 39 bits.
pub const VA_LIMITE: u64 = 1 << 39;

/// Una tabla nueva, a cero. Es memoria del proceso: no entra en la reserva del kernel.
fn tabla_nueva() -> Result<u64, Error> {
    let p = paginas().pagina_usuario().ok_or(Error::SinMemoria)?;
    unsafe { core::ptr::write_bytes(mmu::virt(p as usize) as *mut u8, 0, PAGINA as usize) };
    Ok(p)
}

fn entrada(tabla: u64, i: usize) -> *mut u64 {
    (mmu::virt(tabla as usize) + i * 8) as *mut u64
}

fn indices(va: u64) -> [usize; 3] {
    [((va >> 30) & 0x1ff) as usize, ((va >> 21) & 0x1ff) as usize, ((va >> 12) & 0x1ff) as usize]
}

impl Espacio {
    /// Un espacio vacío. `None` si no quedan ASID o memoria.
    pub fn nuevo(cupo: alloc::sync::Arc<crate::cupo::Cupo>) -> Option<Self> {
        if !cupo.cobrar_suelto(PAGINA) {
            return None;
        }
        let Some(asid) = asid_nuevo() else {
            cupo.devolver(PAGINA);
            return None;
        };
        let Ok(l1) = tabla_nueva() else {
            asid_devolver(asid);
            cupo.devolver(PAGINA);
            return None;
        };
        Some(Self {
            l1,
            asid,
            tablas: alloc::vec::Vec::new(),
            propias: alloc::vec::Vec::new(),
            proyectados: alloc::vec::Vec::new(),
            cupo,
            cobrado: PAGINA,
        })
    }

    /// Cobra una página al grupo (C42).
    fn cobrar(&mut self) -> Result<(), Error> {
        if !self.cupo.cobrar_suelto(PAGINA) {
            return Err(Error::SinMemoria);
        }
        self.cobrado += PAGINA;
        Ok(())
    }

    fn descobrar(&mut self) {
        self.cupo.devolver(PAGINA);
        self.cobrado -= PAGINA;
    }

    pub fn asid(&self) -> u64 {
        self.asid
    }

    /// Valor de TTBR0_EL1 para este espacio.
    pub fn ttbr0(&self) -> u64 {
        self.l1 | (self.asid << 48)
    }

    /// La entrada de nivel 3 de `va`, si ya existen sus tablas.
    fn hoja(&self, va: u64) -> Option<*mut u64> {
        let ix = indices(va);
        let mut tabla = self.l1;
        for &i in &ix[..2] {
            let v = unsafe { entrada(tabla, i).read_volatile() };
            if v & VALIDO != VALIDO {
                return None;
            }
            tabla = v & DIRECCION;
        }
        Some(entrada(tabla, ix[2]))
    }

    /// La entrada de nivel 3 de `va`, creando las tablas que falten.
    fn hoja_o_crear(&mut self, va: u64) -> Result<*mut u64, Error> {
        if !(VA_MINIMA..VA_LIMITE).contains(&va) {
            return Err(Error::Direccion);
        }
        let ix = indices(va);
        let mut tabla = self.l1;
        for &i in &ix[..2] {
            let e = entrada(tabla, i);
            let v = unsafe { e.read_volatile() };
            tabla = if v & VALIDO == VALIDO {
                v & DIRECCION
            } else {
                self.tablas.try_reserve(1).map_err(|_| Error::SinMemoria)?;
                self.cobrar()?;
                let Ok(nueva) = tabla_nueva() else {
                    self.descobrar();
                    return Err(Error::SinMemoria);
                };
                self.tablas.push(nueva);
                unsafe { e.write_volatile(nueva | VALIDO) };
                nueva
            };
        }
        Ok(entrada(tabla, ix[2]))
    }

    /// Comprueba que se pueden proyectar `n` páginas desde `va`: dirección válida, nada proyectado ya y las tablas
    /// creadas. Después, `proyectar` en ese tramo no falla.
    fn preparar(&mut self, va: u64, n: u64) -> Result<(), Error> {
        let fin = va.checked_add(n.checked_mul(PAGINA).ok_or(Error::Direccion)?).ok_or(Error::Direccion)?;
        if va % PAGINA != 0 || va < VA_MINIMA || fin > VA_LIMITE {
            return Err(Error::Direccion);
        }
        let mut p = va;
        while p < fin {
            if self.hoja(p).is_some_and(|e| unsafe { e.read_volatile() } & VALIDO != 0) {
                return Err(Error::Ocupada);
            }
            p += PAGINA;
        }
        let mut p = va;
        while p < fin {
            self.hoja_o_crear(p)?;
            p += PAGINA;
        }
        Ok(())
    }

    /// Proyecta la página física `fisica` en la dirección virtual `va` del proceso, que tiene que estar libre.
    fn proyectar(&mut self, va: u64, fisica: u64, permiso: Permiso) -> Result<(), Error> {
        let e = self.hoja_o_crear(va)?;
        if unsafe { e.read_volatile() } & VALIDO != 0 {
            return Err(Error::Ocupada);
        }
        let atributos = VALIDO | AF | NG | AP_EL0 | PXN
            | match permiso {
                Permiso::Codigo => SH_INTERNO | ATTR_NORMAL | AP_SOLO_LECTURA,
                Permiso::Datos => SH_INTERNO | ATTR_NORMAL | UXN,
                Permiso::SoloLectura => SH_INTERNO | ATTR_NORMAL | UXN | AP_SOLO_LECTURA,
                Permiso::Dispositivo => ATTR_DISPOSITIVO | UXN,
                Permiso::DatosSinCache => SH_INTERNO | ATTR_SIN_CACHE | UXN,
            };
        unsafe {
            e.write_volatile((fisica & DIRECCION) | atributos);
            core::arch::asm!("dsb ishst", "isb");
        }
        Ok(())
    }

    /// Proyecta un objeto de memoria entero desde `va`, de lectura y escritura o de solo lectura; nunca se ejecuta.
    /// Primero comprueba el tramo entero y crea sus tablas, así que o se proyecta todo o nada. El espacio guarda el
    /// objeto mientras exista (C18).
    pub fn proyectar_memoria(&mut self, va: u64, m: &alloc::sync::Arc<super::memoria::Memoria>, escribir: bool) -> Result<(), Error> {
        if self.proyectados.len() >= MAX_PROYECCIONES {
            return Err(Error::Limite);
        }
        self.proyectados.try_reserve(1).map_err(|_| Error::SinMemoria)?;
        self.preparar(va, m.paginas() as u64)?;
        let permiso = match (escribir, m.bus().is_some()) {
            (true, true) => Permiso::DatosSinCache,
            (true, false) => Permiso::Datos,
            (false, _) => Permiso::SoloLectura,
        };
        for (i, &p) in m.fisicas().iter().enumerate() {
            let _ = self.proyectar(va + i as u64 * PAGINA, p, permiso);
        }
        self.proyectados.push((va, m.clone()));
        Ok(())
    }

    /// Quita la proyección que empieza en `va` (C45): borra sus entradas y vacía la TLB de este ASID para cada página,
    /// en todos los núcleos. Si se da `m`, solo si es ese objeto (para revocar sin tocar otra proyección que haya
    /// ocupado el sitio). Falso si no había.
    pub fn quitar_proyeccion(&mut self, va: u64, m: Option<&alloc::sync::Arc<super::memoria::Memoria>>) -> bool {
        let Some(i) = self
            .proyectados
            .iter()
            .position(|(v, o)| *v == va && m.is_none_or(|m| alloc::sync::Arc::ptr_eq(o, m)))
        else {
            return false;
        };
        let (_, objeto) = self.proyectados.swap_remove(i);
        for k in 0..objeto.paginas() as u64 {
            let p = va + k * PAGINA;
            if let Some(e) = self.hoja(p) {
                unsafe {
                    e.write_volatile(0);
                    core::arch::asm!("dsb ishst", "tlbi vae1is, {}", in(reg) (self.asid << 48) | (p >> 12));
                }
            }
        }
        unsafe { core::arch::asm!("dsb ish", "isb") };
        // El objeto se suelta aquí, ya sin entradas que lo vean.
        drop(objeto);
        true
    }

    /// Proyecta registros de aparatos (C20) como memoria de dispositivo, nunca ejecutable. Solo direcciones de la
    /// ventana de periféricos: nunca memoria normal, aunque alguien le pase una.
    pub fn proyectar_dispositivo(&mut self, va: u64, fisica: u64, tamano: u64) -> Result<(), Error> {
        let fin = fisica.checked_add(tamano).ok_or(Error::Direccion)?;
        if fisica < PERIFERICOS || fin > 1 << 32 || fisica % PAGINA != 0 {
            return Err(Error::Direccion);
        }
        self.preparar(va, tamano.div_ceil(PAGINA))?;
        let mut i = 0;
        while i < tamano {
            let _ = self.proyectar(va + i, fisica + i, Permiso::Dispositivo);
            i += PAGINA;
        }
        Ok(())
    }

    /// Reserva una página nueva para el proceso y la proyecta en `va`. Va a cero, con `contenido` copiado a partir de
    /// `desplazamiento` (lo que no quepa en la página se descarta). Si es código, se lleva a la caché de
    /// instrucciones de todos los núcleos.
    pub fn pagina_nueva(&mut self, va: u64, permiso: Permiso, contenido: &[u8], desplazamiento: usize) -> Result<(), Error> {
        self.propias.try_reserve(1).map_err(|_| Error::SinMemoria)?;
        self.cobrar()?;
        let Some(p) = paginas().pagina_usuario() else {
            self.descobrar();
            return Err(Error::SinMemoria);
        };
        let k = mmu::virt(p as usize);
        // La página es nuestra (recién sacada del repartidor) y la mitad alta la ve entera por el mapa lineal.
        let pagina = unsafe { core::slice::from_raw_parts_mut(k as *mut u8, PAGINA as usize) };
        pagina.fill(0);
        let desde = desplazamiento.min(pagina.len());
        let n = contenido.len().min(pagina.len() - desde);
        pagina[desde..desde + n].copy_from_slice(&contenido[..n]);
        if matches!(permiso, Permiso::Codigo) {
            let mut a = k;
            while a < k + PAGINA as usize {
                unsafe { core::arch::asm!("dc cvau, {}", in(reg) a) };
                a += 64;
            }
            unsafe { core::arch::asm!("dsb ish", "ic ialluis", "dsb ish", "isb") };
        }
        if let Err(e) = self.proyectar(va, p, permiso) {
            unsafe { paginas().liberar(p, 1) };
            self.descobrar();
            return Err(e);
        }
        self.propias.push(p);
        Ok(())
    }
}

/// Activa un espacio en este núcleo.
pub(in crate::base) fn activar(ttbr0: u64) {
    unsafe { core::arch::asm!("msr ttbr0_el1, {}", "isb", in(reg) ttbr0) };
}

/// Dirección física de la tabla vacía.
pub fn vacia_fisica() -> u64 {
    mmu::fisica(&VACIA as *const TablaVacia as usize) as u64
}

/// Deja este núcleo sin mitad baja: la tabla vacía, con ASID 0.
pub(in crate::base) fn activar_vacio() {
    activar(mmu::fisica(&VACIA as *const TablaVacia as usize) as u64);
}
