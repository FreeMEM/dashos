//! Espacios de direcciones de los procesos en x86-64 (S30, X4). La misma interfaz que `../espacio.rs` de ARM (C11, S3).
//!
//! Escrito con el manual de Intel (SDM, volumen 3: 4.5, paginación de 4 niveles; 4.10.1, PCID; 4.10.4, invalidar la
//! TLB).
//!
//! La mitad baja de un proceso es PML4[0]: sus 512 GB, los mismos que en ARM (`VA_LIMITE`). Su PML4 lleva además la
//! entrada 511 del kernel, que es la misma tabla para todos: la mitad alta (global, sin bit de usuario). Las tablas son
//! de 4 KB y cuatro niveles.
//!
//! En vez de ASID, PCID de 12 bits (si la CPU lo tiene): al cambiar de proceso no se vacía la TLB. El PCID 0 es el del
//! kernel, cuya mitad baja está vacía (`activar_vacio`, PAN por software como en ARM). Un PCID solo se da a un espacio
//! vivo; al soltarse, se apunta como "sucio" en todos los núcleos, y el siguiente que lo cargue vacía antes lo suyo de
//! la TLB (CR3 sin el bit 63). Así un espacio nuevo nunca hereda traducciones de otro, sin avisar a nadie: nadie corre
//! en un espacio muerto. Al quitar una proyección de un espacio vivo sí hay que avisar a los demás núcleos (puede estar
//! corriendo en otro): `nucleos::vaciar_tlb_otros`. Sin PCID, cada cambio de CR3 ya vacía la TLB de usuario.

use super::nucleos::MAX_NUCLEOS;
use crate::cerrojo::CerrojoIrq;
use crate::memoria::{paginas, PAGINA};
use crate::mmu;
use core::sync::atomic::{AtomicU64, Ordering};

const PCIDS: usize = 4096;
static PCID_OCUPADO: CerrojoIrq<[u64; PCIDS / 64]> = CerrojoIrq::new({
    let mut m = [0u64; PCIDS / 64];
    m[0] = 1;
    m
});
/// Por núcleo, los PCID que hay que vaciar de su TLB la próxima vez que los cargue.
static SUCIO: [[AtomicU64; PCIDS / 64]; MAX_NUCLEOS] = [const { [const { AtomicU64::new(0) }; PCIDS / 64] }; MAX_NUCLEOS];

fn pcid_nuevo() -> Option<u64> {
    let mut m = PCID_OCUPADO.tomar();
    let (i, palabra) = m.iter().enumerate().find(|(_, w)| **w != u64::MAX)?;
    let b = palabra.trailing_ones() as usize;
    m[i] |= 1 << b;
    Some((i * 64 + b) as u64)
}

/// Apunta el PCID como sucio en todos los núcleos (también en este).
fn ensuciar(pcid: u64) {
    let (w, b) = ((pcid / 64) as usize, 1u64 << (pcid % 64));
    for n in SUCIO.iter() {
        n[w].fetch_or(b, Ordering::SeqCst);
    }
}

fn pcid_devolver(pcid: u64) {
    ensuciar(pcid);
    let p = pcid as usize;
    PCID_OCUPADO.tomar()[p / 64] &= !(1 << (p % 64));
}

const PRESENTE: u64 = 1;
const ESCRITURA: u64 = 1 << 1;
const USUARIO: u64 = 1 << 2;
const PWT: u64 = 1 << 3;
const PCD: u64 = 1 << 4;
const ACCEDIDA: u64 = 1 << 5;
const SUCIA: u64 = 1 << 6;
const DIRECCION: u64 = 0x000f_ffff_ffff_f000;
/// Las tablas intermedias lo dejan todo; los permisos van en la hoja.
const TABLA: u64 = PRESENTE | ESCRITURA | USUARIO;
/// CR3 con PCID: no vaciar la TLB de ese PCID al cargarlo.
const SIN_VACIAR: u64 = 1 << 63;

fn nx() -> u64 {
    if super::cpu::hay_nx() { super::mapa::NX } else { 0 }
}

/// Lo que dice `para_copiar` de una página del proceso (como en ARM).
pub enum Copia {
    /// Se puede copiar aquí, por el mapa lineal del kernel.
    Directa(*mut u8),
    /// El anillo 3 no puede (o no está proyectada): la copia falla.
    Falla,
    /// Se puede, pero no por el mapa lineal (registros de un aparato): por el camino de `usuario.rs`.
    Lenta,
}

/// Cómo se proyecta un objeto de memoria (`proyectar_memoria`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Acceso {
    /// Solo leer.
    Leer,
    /// Leer y escribir; nunca se ejecuta.
    Escribir,
    /// Leer y ejecutar; nunca se escribe por esta dirección (H4 y X7a: la memoria de código de un JIT).
    Ejecutar,
}

#[derive(Clone, Copy)]
pub enum Permiso {
    /// Código del proceso: se lee y se ejecuta en el anillo 3.
    Codigo,
    /// Datos y pila: se leen y escriben, no se ejecutan.
    Datos,
    /// Datos que solo se leen.
    SoloLectura,
    /// Registros de un aparato (C20): sin caché (PCD y PWT); nunca se ejecuta.
    Dispositivo,
    /// Búferes de DMA (C46). En un PC el DMA ve lo que hay en la caché de la CPU: van como memoria normal.
    DatosSinCache,
    /// Búferes de DMA proyectados solo para leer.
    SoloLecturaSinCache,
    /// La pantalla del GOP (x86-gpu): con combinación de escritura (WC, la entrada 1 de la PAT: solo PWT), que junta las
    /// escrituras seguidas en ráfagas; sin PAT, memoria normal (los MTRR deciden). Nunca se ejecuta.
    Pantalla,
}

/// Intermediarios revocables por proyección, como mucho: los que puede haber uno dentro de otro (objetos.rs).
pub const VIAS: usize = 4;

/// Páginas como mucho por llamada 58 (`Espacio::paginas_de`), como en ARM.
pub const MAX_PAGINAS_SUELTAS: usize = 256;

/// Por qué no se ponen o sueltan páginas de un objeto de memoria (llamada 58), como en ARM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FalloPaginas {
    /// El tramo se sale del objeto, o son demasiadas páginas de una vez.
    Fuera,
    /// El objeto lo tiene alguien más, o es memoria DMA o una vista.
    Compartida,
    /// No se pudo poner ni la primera: no cabe en el tope de su grupo o no queda memoria.
    Memoria,
}

pub struct Espacio {
    pml4: u64, // dirección física
    pcid: u64,
    /// Páginas de tablas (niveles 3 a 1) y páginas propias del proceso: se devuelven al liberar el espacio (C18).
    tablas: alloc::vec::Vec<u64>,
    propias: alloc::vec::Vec<u64>,
    /// Objetos de memoria proyectados, con la dirección donde empiezan y los intermediarios revocables por los que se
    /// hizo cada uno (como en ARM: C18, C45, M2).
    /// Y el permiso con que se proyectó (para poner una página nueva del objeto, llamada 58).
    proyectados: alloc::vec::Vec<(u64, alloc::sync::Arc<super::memoria::Memoria>, [usize; VIAS], Permiso)>,
    /// El grupo al que se cobran sus tablas y sus páginas propias, y cuánto lleva cobrado (C42).
    cupo: alloc::sync::Arc<crate::cupo::Cupo>,
    cobrado: u64,
}

impl Drop for Espacio {
    fn drop(&mut self) {
        // Ningún núcleo corre ya en este espacio (su único hilo ha muerto): basta con que el siguiente que cargue el
        // PCID vacíe antes su TLB.
        pcid_devolver(self.pcid);
        let mut p = paginas();
        for &t in self.tablas.iter().chain(self.propias.iter()) {
            unsafe { p.liberar(t, 1) };
        }
        unsafe { p.liberar(self.pml4, 1) };
        self.cupo.devolver(self.cobrado);
    }
}

/// Lo que puede fallar al proyectar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// No queda memoria para el proceso (fuera de la reserva del kernel) o para sus tablas.
    SinMemoria,
    /// Ya hay algo proyectado en esa dirección. No se pisa nunca.
    Ocupada,
    /// Demasiadas proyecciones de objetos de memoria (`MAX_PROYECCIONES`).
    Limite,
    /// La dirección no es del usuario o cae en los primeros 64 KB, que no se proyectan nunca.
    Direccion,
}

/// Proyecciones de objetos de memoria por proceso.
const MAX_PROYECCIONES: usize = 1024;

/// Por debajo de esto no se proyecta nada.
pub const VA_MINIMA: u64 = 0x1_0000;
/// Fin de la mitad baja de un proceso: PML4[0], 39 bits como en ARM.
pub const VA_LIMITE: u64 = 1 << 39;

/// Una tabla nueva, a cero. Es memoria del proceso: no entra en la reserva del kernel.
fn tabla_nueva() -> Result<u64, Error> {
    // `pagina_usuario` ya la da a cero.
    crate::memoria::pagina_usuario().ok_or(Error::SinMemoria)
}

fn entrada(tabla: u64, i: usize) -> *mut u64 {
    (mmu::virt(tabla as usize) + i * 8) as *mut u64
}

fn indices(va: u64) -> [usize; 4] {
    [((va >> 39) & 0x1ff) as usize, ((va >> 30) & 0x1ff) as usize, ((va >> 21) & 0x1ff) as usize, ((va >> 12) & 0x1ff) as usize]
}

impl Espacio {
    /// En ARM quita de la caché de instrucciones el código recién cargado (K15). En x86 la caché de instrucciones
    /// sigue sola a las escrituras: nada que hacer.
    pub fn codigo_listo(&mut self) {}

    /// Un espacio vacío. `None` si no quedan PCID o memoria.
    pub fn nuevo(cupo: alloc::sync::Arc<crate::cupo::Cupo>) -> Option<Self> {
        if !cupo.cobrar_suelto(PAGINA) {
            return None;
        }
        let Some(pcid) = pcid_nuevo() else {
            cupo.devolver(PAGINA);
            return None;
        };
        let Ok(pml4) = tabla_nueva() else {
            pcid_devolver(pcid);
            cupo.devolver(PAGINA);
            return None;
        };
        // La mitad alta: la entrada 511 del kernel, la misma tabla para todos (`mapa.rs`).
        let kernel = super::mapa::pml4();
        unsafe { entrada(pml4, 511).write_volatile(entrada(kernel, 511).read_volatile()) };
        Some(Self {
            pml4,
            pcid,
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

    /// Su PCID (el ASID de ARM): de 1 a 4095, distinto del de cualquier otro espacio vivo.
    pub fn asid(&self) -> u64 {
        self.pcid
    }

    /// El valor de CR3 para este espacio: su PML4 y, con PCID, su PCID. Se llama como en ARM (TTBR0).
    pub fn ttbr0(&self) -> u64 {
        self.pml4 | if super::cpu::hay_pcid() { self.pcid } else { 0 }
    }

    /// La entrada de nivel 1 de `va`, si ya existen sus tablas.
    fn hoja(&self, va: u64) -> Option<*mut u64> {
        let ix = indices(va);
        let mut tabla = self.pml4;
        for &i in &ix[..3] {
            let v = unsafe { entrada(tabla, i).read_volatile() };
            if v & PRESENTE == 0 {
                return None;
            }
            tabla = v & DIRECCION;
        }
        Some(entrada(tabla, ix[3]))
    }

    /// Para copiar sin cambiar de espacio y para mirar permisos (como en ARM, M6a): dónde ve el kernel la página de
    /// `va`, por el mapa lineal, si el anillo 3 puede leerla (y escribirla, con `escribir`). Solo memoria normal; con
    /// registros de un aparato, `Lenta`. Hay que tener el espacio tomado mientras se usa lo que devuelve.
    pub fn para_copiar(&self, va: u64, escribir: bool) -> Copia {
        if !(VA_MINIMA..VA_LIMITE).contains(&va) {
            return Copia::Falla;
        }
        let Some(e) = self.hoja(va) else { return Copia::Falla };
        let v = unsafe { e.read_volatile() };
        if v & PRESENTE == 0 || v & USUARIO == 0 || (escribir && v & ESCRITURA == 0) {
            return Copia::Falla;
        }
        let fisica = v & DIRECCION;
        if v & (PCD | PWT) != 0 || !super::mapa::se_ve(fisica, PAGINA) {
            return Copia::Lenta;
        }
        Copia::Directa((mmu::virt(fisica as usize) + (va % PAGINA) as usize) as *mut u8)
    }

    /// ¿Puede el anillo 3 ejecutar en `va`? Para la entrada de un hilo nuevo (llamada 40).
    pub fn ejecutable(&self, va: u64) -> bool {
        if !(VA_MINIMA..VA_LIMITE).contains(&va) {
            return false;
        }
        let Some(e) = self.hoja(va) else { return false };
        // SAFETY: `hoja` da una entrada de una tabla de este espacio, que vive mientras lo tengamos tomado.
        let v = unsafe { e.read_volatile() };
        v & PRESENTE != 0 && v & USUARIO != 0 && v & super::mapa::NX == 0
    }

    /// La palabra de 32 bits de `va` para la espera de hilos (llamadas 42 y 43), como en ARM: su valor, leído de una
    /// vez, y una clave que la nombra (su dirección en el mapa lineal, la misma para dos procesos que comparten la
    /// página). `None` si no está alineada a 4, el anillo 3 no puede leerla o no es memoria normal.
    pub fn palabra(&self, va: u64) -> Option<(u32, usize)> {
        if va % 4 != 0 {
            return None;
        }
        let Copia::Directa(k) = self.para_copiar(va, false) else { return None };
        // SAFETY: `k` está en el mapa lineal, dentro de una página proyectada en este espacio (que tenemos tomado),
        // alineada a 4. Una carga atómica: la escriben a la vez los hilos del proceso.
        let v = unsafe { (*(k as *const core::sync::atomic::AtomicU32)).load(core::sync::atomic::Ordering::SeqCst) };
        Some((v, k as usize))
    }

    /// Los permisos de la entrada de `va` para el anillo 3: (leer, escribir, ejecutar). Para las pruebas.
    #[cfg(feature = "pruebas")]
    pub fn permisos_el0(&self, va: u64) -> Option<(bool, bool, bool)> {
        let e = self.hoja(va)?;
        let v = unsafe { e.read_volatile() };
        if v & PRESENTE == 0 || v & USUARIO == 0 {
            return None;
        }
        Some((true, v & ESCRITURA != 0, v & super::mapa::NX == 0))
    }

    /// La entrada de nivel 1 de `va`, creando las tablas que falten.
    fn hoja_o_crear(&mut self, va: u64) -> Result<*mut u64, Error> {
        if !(VA_MINIMA..VA_LIMITE).contains(&va) {
            return Err(Error::Direccion);
        }
        self.hoja_o_crear_en(va)
    }

    /// Como `hoja_o_crear`, sin mirar `VA_MINIMA`: solo para la página 0 de la caja Amiga (`proyectar_pagina_cero`).
    fn hoja_o_crear_en(&mut self, va: u64) -> Result<*mut u64, Error> {
        let ix = indices(va);
        let mut tabla = self.pml4;
        for &i in &ix[..3] {
            let e = entrada(tabla, i);
            let v = unsafe { e.read_volatile() };
            tabla = if v & PRESENTE != 0 {
                v & DIRECCION
            } else {
                self.tablas.try_reserve(1).map_err(|_| Error::SinMemoria)?;
                self.cobrar()?;
                let Ok(nueva) = tabla_nueva() else {
                    self.descobrar();
                    return Err(Error::SinMemoria);
                };
                self.tablas.push(nueva);
                unsafe { e.write_volatile(nueva | TABLA) };
                nueva
            };
        }
        Ok(entrada(tabla, ix[3]))
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
            if self.hoja(p).is_some_and(|e| unsafe { e.read_volatile() } & PRESENTE != 0) {
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

    /// Proyecta la página física `fisica` en la dirección virtual `va` del proceso, que tiene que estar libre. Una
    /// entrada nueva no pide vaciar ninguna TLB: la CPU no guarda las que no están presentes.
    fn proyectar(&mut self, va: u64, fisica: u64, permiso: Permiso) -> Result<(), Error> {
        let e = self.hoja_o_crear(va)?;
        if unsafe { e.read_volatile() } & PRESENTE != 0 {
            return Err(Error::Ocupada);
        }
        let atributos = PRESENTE | USUARIO | ACCEDIDA
            | match permiso {
                Permiso::Codigo => 0,
                Permiso::Datos | Permiso::DatosSinCache => ESCRITURA | SUCIA | nx(),
                Permiso::SoloLectura | Permiso::SoloLecturaSinCache => nx(),
                Permiso::Dispositivo => ESCRITURA | SUCIA | PCD | PWT | nx(),
                Permiso::Pantalla => ESCRITURA | SUCIA | (if super::cpu::hay_wc() { PWT } else { 0 }) | nx(),
            };
        unsafe { e.write_volatile((fisica & DIRECCION) | atributos) };
        Ok(())
    }

    /// La página 0 de la caja Amiga (H4, llamada 49), como en ARM: un objeto de una sola página en la dirección 0, de
    /// solo lectura y nunca ejecutable. Lo demás de los primeros 64 KB sigue sin proyectarse. Dentro del kernel la mitad
    /// baja está vacía y SMAP cierra lo del anillo 3: un puntero nulo del kernel sigue fallando.
    pub fn proyectar_pagina_cero(&mut self, m: &alloc::sync::Arc<super::memoria::Memoria>) -> Result<(), Error> {
        if m.paginas() != 1 || m.bus().is_some() {
            return Err(Error::Direccion);
        }
        if self.proyectados.len() >= MAX_PROYECCIONES {
            return Err(Error::Limite);
        }
        self.proyectados.try_reserve(1).map_err(|_| Error::SinMemoria)?;
        let e = self.hoja_o_crear_en(0)?;
        if unsafe { e.read_volatile() } & PRESENTE != 0 {
            return Err(Error::Ocupada);
        }
        let f = m.fisica(0);
        if f == 0 {
            return Err(Error::Direccion);
        }
        unsafe { e.write_volatile((f & DIRECCION) | PRESENTE | USUARIO | ACCEDIDA | nx()) };
        self.proyectados.push((0, m.clone(), [0; VIAS], Permiso::SoloLectura));
        Ok(())
    }

    /// Proyecta un objeto de memoria entero desde `va`: de solo lectura, de lectura y escritura o (H4, X7a: la memoria
    /// de código de un JIT) de lectura y ejecución, nunca de escritura y ejecución a la vez. El mismo objeto puede
    /// estar proyectado para escribir en una dirección y para ejecutar en otra. O todo o nada. El espacio guarda el
    /// objeto mientras esté proyectado (C18). En x86 la caché de instrucciones sigue sola a las escrituras.
    pub fn proyectar_memoria(&mut self, va: u64, m: &alloc::sync::Arc<super::memoria::Memoria>, acceso: Acceso, via: &[usize]) -> Result<(), Error> {
        if via.len() > VIAS {
            return Err(Error::Limite);
        }
        // La memoria de DMA no se ejecuta nunca: la escribe un aparato. Una vista (llamada 50), solo se lee.
        if acceso == Acceso::Ejecutar && m.bus().is_some() || acceso != Acceso::Leer && m.es_vista() {
            return Err(Error::Direccion);
        }
        if self.proyectados.len() >= MAX_PROYECCIONES {
            return Err(Error::Limite);
        }
        self.proyectados.try_reserve(1).map_err(|_| Error::SinMemoria)?;
        self.preparar(va, m.paginas() as u64)?;
        let permiso = match (acceso, m.bus().is_some()) {
            (Acceso::Escribir, true) => Permiso::DatosSinCache,
            (Acceso::Escribir, false) => Permiso::Datos,
            (Acceso::Leer, true) => Permiso::SoloLecturaSinCache,
            (Acceso::Leer, false) => Permiso::SoloLectura,
            (Acceso::Ejecutar, _) => Permiso::Codigo,
        };
        // Las páginas soltadas por el globo de una máquina (0) no se proyectan; en x86 no hay máquinas todavía.
        for (i, p) in m.fisicas().enumerate().filter(|&(_, p)| p != 0) {
            let _ = self.proyectar(va + i as u64 * PAGINA, p, permiso);
        }
        let mut vias = [0usize; VIAS];
        vias[..via.len()].copy_from_slice(via);
        self.proyectados.push((va, m.clone(), vias, permiso));
        Ok(())
    }

    /// ¿Hay en `va` una proyección hecha a través del intermediario `via`? (para purgar sus listas, M2)
    pub fn proyectada_via(&self, va: u64, via: usize) -> bool {
        self.proyectados.iter().any(|(v, _, vs, _)| *v == va && vs.contains(&via))
    }

    /// Quita la proyección que empieza en `va` (C45), con las mismas condiciones que en ARM (`m`, `via`). Borra sus
    /// entradas y las saca de la TLB de todos los núcleos antes de soltar el objeto: en este, la próxima vez que se
    /// cargue el PCID; en los demás, ya (el proceso puede estar corriendo en otro). Falso si no había.
    pub fn quitar_proyeccion(&mut self, va: u64, m: Option<&alloc::sync::Arc<super::memoria::Memoria>>, via: Option<usize>) -> bool {
        let Some(i) = self.proyectados.iter().position(|(v, o, vs, _)| {
            *v == va && m.is_none_or(|m| alloc::sync::Arc::ptr_eq(o, m)) && via.is_none_or(|x| vs.contains(&x))
        }) else {
            return false;
        };
        let (_, objeto, _, _) = self.proyectados.swap_remove(i);
        for k in 0..objeto.paginas() as u64 {
            if let Some(e) = self.hoja(va + k * PAGINA) {
                unsafe { e.write_volatile(0) };
            }
        }
        ensuciar(self.pcid);
        super::nucleos::vaciar_tlb_otros();
        // El objeto se suelta aquí, ya sin entradas que lo vean.
        drop(objeto);
        true
    }

    /// Cuántas proyecciones de `m` tiene (para saber si alguien más lo tiene, llamada 58).
    pub fn proyecciones_de(&self, m: &alloc::sync::Arc<super::memoria::Memoria>) -> usize {
        self.proyectados.iter().filter(|(_, o, _, _)| alloc::sync::Arc::ptr_eq(o, m)).count()
    }

    /// Llamada 58, como en ARM (`../espacio.rs`, `paginas_de`): pone (`poner`) o suelta las páginas
    /// `primera..primera + n` del objeto `m` (`n` hasta [`MAX_PAGINAS_SUELTAS`]) en las proyecciones de este espacio, y
    /// dice cuántas quedaron como se pedía desde la primera. Solo con un objeto que no tiene nadie más que este proceso
    /// (sus `Arc` son las proyecciones de aquí más `mias`), ni memoria DMA ni una vista. Al soltar, las entradas se
    /// borran y salen de la TLB de todos los núcleos (este, al volver al proceso; los demás, ya) antes de devolver las
    /// páginas al sistema.
    pub fn paginas_de(&mut self, m: &alloc::sync::Arc<super::memoria::Memoria>, primera: usize, n: usize, poner: bool, mias: usize) -> Result<usize, FalloPaginas> {
        if n == 0 || n > MAX_PAGINAS_SUELTAS || primera.checked_add(n).is_none_or(|fin| fin > m.paginas()) {
            return Err(FalloPaginas::Fuera);
        }
        if m.bus().is_some() || m.es_vista() || alloc::sync::Arc::strong_count(m) != self.proyecciones_de(m) + mias {
            return Err(FalloPaginas::Compartida);
        }
        let mut hechas = 0;
        if poner {
            for k in primera..primera + n {
                if m.fisica(k) == 0 {
                    let Some(f) = crate::memoria::pagina_usuario() else { break };
                    if !m.reponer_pagina(k, f) {
                        // SAFETY: recién sacada del repartidor y sin poner en ningún sitio: nadie más la tiene.
                        unsafe { paginas().liberar(f, 1) };
                        break;
                    }
                    // Sus tablas ya existen (se crearon al proyectar el objeto entero) y la entrada está vacía: una
                    // entrada nueva no pide vaciar ninguna TLB.
                    let destinos: alloc::vec::Vec<(u64, Permiso)> =
                        self.proyectados.iter().filter(|(_, o, _, _)| alloc::sync::Arc::ptr_eq(o, m)).map(|(va, _, _, p)| (*va + k as u64 * PAGINA, *p)).collect();
                    for (va, permiso) in destinos {
                        let _ = self.proyectar(va, f, permiso);
                    }
                }
                hechas += 1;
            }
            if hechas == 0 {
                return Err(FalloPaginas::Memoria);
            }
            return Ok(hechas);
        }
        let mut sueltas = [0u64; MAX_PAGINAS_SUELTAS];
        let mut n_sueltas = 0;
        let mut quitadas = false;
        for k in primera..primera + n {
            if m.fisica(k) != 0 {
                for i in 0..self.proyectados.len() {
                    let (va, ref o, _, _) = self.proyectados[i];
                    if alloc::sync::Arc::ptr_eq(o, m)
                        && let Some(e) = self.hoja(va + k as u64 * PAGINA)
                    {
                        unsafe { e.write_volatile(0) };
                        quitadas = true;
                    }
                }
            }
        }
        if quitadas {
            // Primero fuera de las TLB: en este núcleo, al volver al proceso (su PCID, sucio); en los demás, ya.
            ensuciar(self.pcid);
            super::nucleos::vaciar_tlb_otros();
        }
        for k in primera..primera + n {
            if m.fisica(k) != 0
                && let Some(f) = m.soltar_pagina(k)
            {
                sueltas[n_sueltas] = f;
                n_sueltas += 1;
            }
            hechas += 1;
        }
        let mut p = paginas();
        for &f in &sueltas[..n_sueltas] {
            // SAFETY: eran del objeto, que solo tiene este proceso (contado arriba, con su tabla de llaves tomada por
            // quien llama): ya no están en sus tablas ni en ninguna TLB, y `soltar_pagina` las sacó del objeto.
            unsafe { p.liberar(f, 1) };
        }
        Ok(hechas)
    }

    /// Proyecta memoria del firmware (C50; en un PC, la pantalla del GOP): fuera de la RAM que reparte el kernel, o
    /// dentro de una de sus reservas. Con combinación de escritura (x86-gpu, `Permiso::Pantalla`): con los MTRR del
    /// firmware, que suelen dejar la pantalla sin caché (UC), WC gana (SDM, volumen 3, tabla 13-7), y la copia del
    /// compositor va en ráfagas en vez de palabra a palabra. Nunca ejecutable.
    pub fn proyectar_firmware(&mut self, va: u64, fisica: u64, tamano: u64) -> Result<(), Error> {
        fisica.checked_add(tamano).ok_or(Error::Direccion)?;
        let p = paginas();
        let permitido = p.fuera_de_la_ram(fisica, tamano) || p.dentro_de_una_reserva(fisica, tamano);
        drop(p);
        if fisica % PAGINA != 0 || tamano % PAGINA != 0 || !permitido {
            return Err(Error::Direccion);
        }
        self.preparar(va, tamano.div_ceil(PAGINA))?;
        let mut i = 0;
        while i < tamano {
            let _ = self.proyectar(va + i, fisica + i, Permiso::Pantalla);
            i += PAGINA;
        }
        Ok(())
    }

    /// Proyecta registros de aparatos (C20) sin caché, nunca ejecutables. Solo fuera de la RAM: nunca memoria normal,
    /// aunque alguien le pase una.
    pub fn proyectar_dispositivo(&mut self, va: u64, fisica: u64, tamano: u64) -> Result<(), Error> {
        fisica.checked_add(tamano).ok_or(Error::Direccion)?;
        if fisica % PAGINA != 0 || !paginas().fuera_de_la_ram(fisica, tamano.div_ceil(PAGINA) * PAGINA) {
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
    /// `desplazamiento` (lo que no quepa en la página se descarta). En x86 la caché de instrucciones sigue sola a la
    /// de datos: no hay nada que limpiar para el código.
    pub fn pagina_nueva(&mut self, va: u64, permiso: Permiso, contenido: &[u8], desplazamiento: usize) -> Result<(), Error> {
        self.propias.try_reserve(1).map_err(|_| Error::SinMemoria)?;
        self.cobrar()?;
        let Some(p) = crate::memoria::pagina_usuario() else {
            self.descobrar();
            return Err(Error::SinMemoria);
        };
        let k = mmu::virt(p as usize);
        // La página es nuestra (recién sacada del repartidor) y la mitad alta la ve entera por el mapa lineal.
        // Ya viene a cero (`pagina_usuario`).
        let pagina = unsafe { core::slice::from_raw_parts_mut(k as *mut u8, PAGINA as usize) };
        let desde = desplazamiento.min(pagina.len());
        let n = contenido.len().min(pagina.len() - desde);
        pagina[desde..desde + n].copy_from_slice(&contenido[..n]);
        if let Err(e) = self.proyectar(va, p, permiso) {
            unsafe { paginas().liberar(p, 1) };
            self.descobrar();
            return Err(e);
        }
        self.propias.push(p);
        Ok(())
    }
}

/// Activa un espacio (un valor de `Espacio::ttbr0`) en este núcleo. Con 0, la mitad baja vacía. Con PCID, sin vaciar
/// la TLB salvo que su PCID esté sucio en este núcleo.
pub(in crate::base) fn activar(cr3: u64) {
    if cr3 == 0 {
        activar_vacio();
        return;
    }
    let ahora = super::cpu::cr3();
    if super::cpu::hay_pcid() {
        let pcid = cr3 & 0xfff;
        let n = super::nucleos::numero() as usize % MAX_NUCLEOS;
        let (w, b) = ((pcid / 64) as usize, 1u64 << (pcid % 64));
        if SUCIO[n][w].load(Ordering::SeqCst) & b != 0 {
            SUCIO[n][w].fetch_and(!b, Ordering::SeqCst);
            cargar_cr3(cr3);
        } else if ahora != cr3 {
            cargar_cr3(cr3 | SIN_VACIAR);
        }
    } else if ahora != cr3 {
        cargar_cr3(cr3);
    }
}

fn cargar_cr3(v: u64) {
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) v, options(nostack, preserves_flags)) };
}

/// Deja este núcleo sin mitad baja: el PML4 del kernel, con el PCID 0 (cuya mitad baja nunca tiene nada).
pub(in crate::base) fn activar_vacio() {
    let kernel = super::mapa::pml4();
    if kernel == 0 || super::cpu::cr3() & DIRECCION == kernel {
        return;
    }
    cargar_cr3(kernel | if super::cpu::hay_pcid() { SIN_VACIAR } else { 0 });
}

/// Dirección física de la tabla de la mitad baja vacía: el PML4 del kernel.
pub fn vacia_fisica() -> u64 {
    super::mapa::pml4()
}
