//! El puente de Wayland de la caja Linux, lado de DashOS (H9, G1 de `docs/GRAFICOS_LOOP.md`).
//!
//! Dentro de Arch corre `puente` (`puente/src/bin/puente.rs`): hace de compositor para los programas de Linux y lo pasa
//! todo por vsock al puerto `PUERTO` de DashOS. Aquí, cada conexión de vsock a ese puerto es un
//! `wayland::puente::Puente`: sus clientes son subconexiones del compositor (por la llave "ventanas" de la caja) y sus
//! memorias, objetos de memoria de DashOS que el compositor lee con una copia de solo lectura de su llave.
//!
//! **Sin copia**: los pools de `wl_shm` que el puente de Linux da por sus páginas físicas (`MEMORIA_VISTA`) son vistas
//! de la RAM del invitado (llamada 50): cada dirección se comprueba contra la RAM (alineada, dentro de los trozos de
//! `TROZO` que tiene la máquina) y se pasa a número de página de esos trozos. El compositor solo puede leerla. Si el
//! invitado cambia el búfer mientras se compone, solo se estropea su ventana; si nombra otras páginas suyas, ve en su
//! ventana lo que ya era suyo.
//!
//! Los hilos: los de las vCPU llaman al aparato vsock con el cerrojo de la caja tomado, y él a `ExtremoPuente` (que toma
//! el cerrojo del puente, `PUENTE`). Un hilo propio lee lo que contesta el compositor (un solo canal para todas las
//! subconexiones), lo deja en el puente de su transporte (solo con `PUENTE`) y despierta a la vCPU 0 para que el
//! aparato lo pase al invitado. Nunca se toma el cerrojo de la caja con el del puente tomado.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use caja_vmm::virtio::vsock::ExtremoVsock;
use dashos::println;
use wayland::puente::{MAX_MEMORIAS, MAX_MEMORIA, Puente, Sistema};

use super::{COMPARTIDO, Compartida};

/// El puerto de vsock en el que escucha DashOS.
pub const PUERTO: u32 = 5000;
/// Transportes a la vez (el puente de Arch y, en las pruebas, el hostil).
const MAX_TRANSPORTES: usize = 8;
/// Dónde se proyectan las memorias de cada transporte: `MAX_MEMORIAS` huecos de 16 MB desde aquí, uno tras otro.
const VA_PUENTE: usize = 0x50_0000_0000;
/// Dónde se lee una memoria que manda el compositor (el mapa de teclado).
const VA_LEER: usize = 0x4f_0000_0000;
const PILA_PUENTE: usize = 64 * 1024;
/// Lo que mide cada objeto de memoria de la RAM del invitado.
const TROZO: usize = 16 << 20;

/// Lo que el puente pide al sistema, para el transporte del hueco `hueco`.
struct SistemaCaja {
    hueco: usize,
    /// La RAM del invitado: sus objetos de memoria (de `TROZO` bytes, seguidos) y la dirección física del invitado donde
    /// empieza. Vacía si no hay vistas.
    ram: Vec<u32>,
    base_ram: u64,
    ventanas: u32,
    /// El extremo del canal de respuesta que se da al compositor (una copia por subconexión).
    respuesta: u32,
    /// Llave → dónde está proyectada para escribirla.
    proyectadas: BTreeMap<u32, usize>,
    libres: Vec<usize>,
    siguiente: usize,
}

impl SistemaCaja {
    fn hueco_libre(&mut self) -> Option<usize> {
        self.libres.pop().or_else(|| {
            (self.siguiente < MAX_MEMORIAS).then(|| {
                self.siguiente += 1;
                VA_PUENTE + (self.hueco * MAX_MEMORIAS + self.siguiente - 1) * MAX_MEMORIA as usize
            })
        })
    }
}

impl Sistema for SistemaCaja {
    fn memoria_nueva(&mut self, tam: u64) -> Option<u32> {
        let va = self.hueco_libre()?;
        let l = dashos::memoria_crear(tam as usize) as i32;
        if l <= 0 {
            self.libres.push(va);
            return None;
        }
        if dashos::memoria_proyectar(l as u32, va, true) != 0 {
            dashos::cerrar(l as u32);
            self.libres.push(va);
            return None;
        }
        self.proyectadas.insert(l as u32, va);
        Some(l as u32)
    }

    fn memoria_escribir(&mut self, llave: u32, desde: u64, datos: &[u8]) {
        if let Some(&va) = self.proyectadas.get(&llave) {
            // SAFETY: el objeto está proyectado en `va` para escribir y mide al menos `desde + datos.len()` (lo comprueba
            // `Puente` con el tamaño que pidió al crearlo).
            unsafe { core::ptr::copy_nonoverlapping(datos.as_ptr(), (va + desde as usize) as *mut u8, datos.len()) };
        }
    }

    fn memoria_leer(&mut self, llave: u32, n: usize) -> Option<Vec<u8>> {
        if (dashos::memoria_tamano(llave) as i64) < n as i64 || dashos::memoria_proyectar(llave, VA_LEER, false) != 0 {
            return None;
        }
        let mut v = alloc::vec![0u8; n];
        // SAFETY: recién proyectado en `VA_LEER`, solo de lectura, y mide al menos `n`.
        unsafe { core::ptr::copy_nonoverlapping(VA_LEER as *const u8, v.as_mut_ptr(), n) };
        dashos::memoria_quitar(VA_LEER);
        Some(v)
    }

    fn memoria_tamano(&mut self, llave: u32) -> Option<u64> {
        let t = dashos::memoria_tamano(llave);
        (t >= 0).then_some(t as u64)
    }

    fn copia_lectura(&mut self, llave: u32) -> Option<u32> {
        let c = dashos::llave_duplicar(llave, dashos::LEER | dashos::TRANSFERIR, 0);
        (c > 0).then_some(c as u32)
    }

    fn memoria_vista(&mut self, direcciones: &[u64]) -> Option<u32> {
        let paginas_ram = (self.ram.len() * TROZO / 4096) as u64;
        let mut numeros = Vec::new();
        numeros.try_reserve_exact(direcciones.len()).ok()?;
        for &d in direcciones {
            let p = d.checked_sub(self.base_ram).filter(|p| p % 4096 == 0)? / 4096;
            if p >= paginas_ram {
                return None;
            }
            numeros.push(p as u32);
        }
        let l = dashos::memoria_vista(&self.ram, &numeros);
        (l > 0).then_some(l as u32)
    }

    fn copia_respuesta(&mut self) -> Option<u32> {
        let c = dashos::llave_duplicar(self.respuesta, dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
        (c > 0).then_some(c as u32)
    }

    fn cerrar(&mut self, llave: u32) {
        if let Some(va) = self.proyectadas.remove(&llave) {
            dashos::memoria_quitar(va);
            self.libres.push(va);
        }
        dashos::cerrar(llave);
    }

    fn al_compositor(&mut self, bytes: &[u8], llaves: &[u32]) -> Result<(), bool> {
        match dashos::canal_escribir(self.ventanas, bytes, llaves) {
            0 => {
                // Las llaves mandadas ya no son de aquí.
                Ok(())
            }
            dashos::E_CERRADO => Err(false),
            _ => Err(true),
        }
    }
}

/// Un transporte en marcha: la conexión de vsock y su puente.
struct Transporte {
    con: u32,
    puente: Puente<SistemaCaja>,
}

pub struct EstadoPuente {
    ventanas: u32,
    ram: Vec<u32>,
    base_ram: u64,
    respuesta: u32,
    huecos: [Option<Transporte>; MAX_TRANSPORTES],
    /// Conexiones que hay que cortar (el flujo no era de tramas).
    cortar: Vec<u32>,
    /// Puentes de transportes ya cerrados con algo aún por mandar al compositor (los adioses de sus clientes): el hilo
    /// los vacía. Si no, sus ventanas se quedarían en pantalla.
    cerrandose: Vec<Puente<SistemaCaja>>,
    /// Cuentas de los transportes que ya acabaron (para el resumen).
    pub acabadas: wayland::puente::Cuentas,
    pub transportes: u64,
    pub cortados: u64,
}

impl EstadoPuente {
    fn hueco_de(&mut self, con: u32) -> Option<&mut Transporte> {
        self.huecos.iter_mut().flatten().find(|t| t.con == con)
    }

    /// Quita el transporte de `con`: sus clientes fuera del compositor, sus memorias soltadas y sus cuentas al resumen.
    fn quitar(&mut self, con: u32) -> Option<wayland::puente::Cuentas> {
        let h = self.huecos.iter_mut().find(|h| h.as_ref().is_some_and(|t| t.con == con))?;
        let mut t = h.take()?;
        t.puente.cerrar_todo();
        let c = t.puente.cuentas;
        sumar(&mut self.acabadas, &c);
        if t.puente.atrasado() {
            self.cerrandose.push(t.puente);
        }
        Some(c)
    }

    /// Las cuentas de todos, los vivos y los acabados.
    pub fn cuentas(&self) -> wayland::puente::Cuentas {
        let mut c = self.acabadas;
        for t in self.huecos.iter().flatten() {
            sumar(&mut c, &t.puente.cuentas);
        }
        c
    }
}

fn sumar(a: &mut wayland::puente::Cuentas, b: &wayland::puente::Cuentas) {
    a.clientes += b.clientes;
    a.bytes_al_compositor += b.bytes_al_compositor;
    a.bytes_al_invitado += b.bytes_al_invitado;
    a.bytes_de_memoria += b.bytes_de_memoria;
    a.memorias += b.memorias;
    a.vistas += b.vistas;
    a.vistas_no += b.vistas_no;
    a.malas += b.malas;
    a.cerrados += b.cerrados;
}

pub static PUENTE: Compartida<Option<EstadoPuente>> = Compartida::nueva(None);
/// El extremo de lectura del canal de respuesta (para el hilo).
static LECTURA: AtomicU32 = AtomicU32::new(0);

/// Pone en marcha el lado de DashOS: el canal de respuesta y su hilo. Falso si no se pudo. `ram` son los objetos de
/// memoria de la RAM del invitado (de 16 MB, seguidos desde `base_ram`): con ellos, los búferes van sin copia.
pub fn abrir(ventanas: u32, ram: &[u32], base_ram: u64) -> bool {
    let (lectura, respuesta) = dashos::canal_crear();
    LECTURA.store(lectura, Ordering::Release);
    PUENTE.con(|p| {
        *p = Some(EstadoPuente {
            ventanas,
            ram: ram.to_vec(),
            base_ram,
            respuesta,
            huecos: Default::default(),
            cortar: Vec::new(),
            cerrandose: Vec::new(),
            acabadas: Default::default(),
            transportes: 0,
            cortados: 0,
        })
    });
    let pila: &'static mut [u8] = Box::leak(alloc::vec![0u8; PILA_PUENTE].into_boxed_slice());
    dashos::hilos::crear(hilo_puente, 0, pila, 0) >= 0
}

/// Lee lo que contesta el compositor a los clientes del invitado y lo deja en el puente de su transporte. Si hay algo
/// atrasado hacia el compositor, mira cada 5 ms y despierta a la vCPU para reintentar (el aparato llama al puente).
fn hilo_puente(_: u64) -> u64 {
    let lectura = LECTURA.load(Ordering::Acquire);
    let mut b = [0u8; wayland::canal::MAX_CANAL];
    let mut ll = [0u32; wayland::canal::MAX_LLAVES];
    loop {
        let (n, k, _) = dashos::canal_leer_plazo(lectura, &mut b, &mut ll, 5_000);
        if n == dashos::E_CERRADO {
            return 0;
        }
        let mut avisar = false;
        PUENTE.con(|p| {
            let Some(p) = p.as_mut() else { return };
            if n >= 0 {
                let trozo = &b[..n as usize];
                let llaves = &ll[..k];
                let prefijo = wayland::canal::partir_sub(trozo).map(|(s, _)| s >> 16);
                match prefijo.and_then(|x| p.huecos.iter_mut().flatten().find(|t| t.puente.prefijo() == x)) {
                    Some(t) => {
                        t.puente.del_compositor(trozo, llaves);
                        avisar |= t.puente.hay_salida();
                    }
                    None => llaves.iter().for_each(|&l| {
                        dashos::cerrar(l);
                    }),
                }
            }
            for t in p.huecos.iter_mut().flatten() {
                if t.puente.atrasado() {
                    t.puente.vaciar();
                    avisar = true;
                }
            }
            for c in p.cerrandose.iter_mut() {
                c.vaciar();
            }
            p.cerrandose.retain(|c| c.atrasado() && !c.sin_compositor);
        });
        if avisar {
            COMPARTIDO.avisar();
        }
    }
}

/// El extremo de vsock: conecta el aparato con los puentes.
pub struct ExtremoPuente;

impl ExtremoVsock for ExtremoPuente {
    fn conectar(&mut self, con: u32, puerto: u32) -> bool {
        if puerto != PUERTO {
            return false;
        }
        PUENTE.con(|p| {
            let Some(p) = p.as_mut() else { return false };
            let Some(i) = p.huecos.iter().position(Option::is_none) else {
                println!("  [caja-linux] wayland: más de {} transportes a la vez: no", MAX_TRANSPORTES);
                return false;
            };
            let sis = SistemaCaja { hueco: i, ram: p.ram.clone(), base_ram: p.base_ram, ventanas: p.ventanas, respuesta: p.respuesta, proyectadas: BTreeMap::new(), libres: Vec::new(), siguiente: 0 };
            // El prefijo, distinto en cada transporte: el hueco más 8 por cada vuelta (no se repite pronto).
            p.transportes += 1;
            let prefijo = (i + MAX_TRANSPORTES * (p.transportes as usize % 15)) as u32;
            let vistas = !p.ram.is_empty();
            let mut puente = Puente::nuevo(sis, prefijo);
            if vistas {
                puente.anunciar_vistas();
            }
            p.huecos[i] = Some(Transporte { con, puente });
            println!("  [caja-linux] wayland: el puente de Linux conecta por vsock (transporte {})", p.transportes);
            true
        })
    }

    fn recibir(&mut self, con: u32, datos: &[u8]) -> usize {
        PUENTE.con(|p| {
            let Some(p) = p.as_mut() else { return datos.len() };
            let Some(t) = p.hueco_de(con) else { return datos.len() };
            match t.puente.recibir(datos) {
                Ok(n) => n,
                Err(e) => {
                    println!("  [caja-linux] wayland: el puente de Linux manda algo que no son tramas ({:?}): se corta", e);
                    p.quitar(con);
                    p.cortados += 1;
                    p.cortar.push(con);
                    datos.len()
                }
            }
        })
    }

    fn cerrada(&mut self, con: u32) {
        PUENTE.con(|p| {
            let Some(p) = p.as_mut() else { return };
            if let Some(c) = p.quitar(con) {
                println!(
                    "  [caja-linux] wayland: el transporte se cierra: {} clientes, {} KB de Wayland al compositor y {} al invitado, {} KB de búferes copiados, {} memorias sin copia, {} tramas malas",
                    c.clientes, c.bytes_al_compositor / 1024, c.bytes_al_invitado / 1024, c.bytes_de_memoria / 1024, c.vistas, c.malas
                );
            }
        })
    }

    fn enviar(&mut self, con: u32, buf: &mut [u8]) -> usize {
        PUENTE.con(|p| p.as_mut().and_then(|p| p.hueco_de(con)).map_or(0, |t| t.puente.enviar(buf)))
    }

    fn hay_algo(&self) -> bool {
        PUENTE.con(|p| p.as_ref().is_some_and(|p| !p.cortar.is_empty() || p.huecos.iter().flatten().any(|t| t.puente.hay_salida())))
    }

    fn cerrar(&mut self) -> Option<u32> {
        PUENTE.con(|p| p.as_mut().and_then(|p| p.cortar.pop()))
    }
}

