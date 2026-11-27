//! Banco de pruebas: una caja falsa (memoria, reservas, un proceso, consola) y un servicio de archivos en memoria con
//! un espía que comprueba lo que dos le manda.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use caja_dos::estructuras::*;
use caja_dos::fs::*;
use caja_dos::*;

pub const TAM_MEMORIA: usize = 8 << 20;
const PRIMERA_LIBRE: u32 = 0x10000;

/// La caja de las pruebas. Las reservas no se reusan; liberar lo que no está reservado, o con otro tamaño, rompe la
/// prueba (así se ve si dos suelta algo mal).
pub struct CajaPrueba {
    pub mem: MemoriaVec,
    siguiente: u32,
    pub vivos: BTreeMap<u32, u32>,
    pub tarea: u32,
    pub salida: Vec<u8>,
    pub entrada: VecDeque<u8>,
    pub hora: u64,
    pub senales: u32,
    /// Lo que se pasó al cargador, y el BPTR que devuelve.
    pub cargado: Vec<Vec<u8>>,
    pub descargado: Vec<u32>,
    /// Si es `Some`, reservar falla cuando quedan 0.
    pub reservas_restantes: Option<u32>,
}

impl Memoria for CajaPrueba {
    fn trozo(&self, dir: u32, largo: u32) -> Option<&[u8]> {
        self.mem.trozo(dir, largo)
    }
    fn trozo_mut(&mut self, dir: u32, largo: u32) -> Option<&mut [u8]> {
        self.mem.trozo_mut(dir, largo)
    }
}

impl Caja for CajaPrueba {
    fn reservar(&mut self, largo: u32) -> u32 {
        if let Some(n) = &mut self.reservas_restantes {
            if *n == 0 {
                return 0;
            }
            *n -= 1;
        }
        let d = (self.siguiente + 7) & !7;
        let fin = d as usize + largo as usize;
        if fin + 0x1000 > TAM_MEMORIA {
            return 0;
        }
        self.siguiente = fin as u32;
        self.mem.datos[d as usize..fin].fill(0);
        self.vivos.insert(d, largo);
        d
    }
    fn liberar(&mut self, dir: u32, largo: u32) {
        match self.vivos.remove(&dir) {
            Some(l) if l == largo => {
                // Se ensucia para que un uso después de liberar se note.
                self.mem.datos[dir as usize..(dir + largo) as usize].fill(0xEE);
            }
            Some(l) => panic!("liberar {dir:#x} con {largo} bytes, reservado con {l}"),
            None => panic!("liberar {dir:#x} ({largo} bytes), que no estaba reservado"),
        }
    }
    fn tarea_actual(&self) -> u32 {
        self.tarea
    }
    fn hora(&self) -> (u64, u32) {
        (self.hora, 0)
    }
    fn consola_escribir(&mut self, datos: &[u8]) -> usize {
        self.salida.extend_from_slice(datos);
        datos.len()
    }
    fn consola_leer(&mut self, buf: &mut [u8]) -> usize {
        let mut n = 0;
        while n < buf.len() {
            match self.entrada.pop_front() {
                Some(b) => {
                    buf[n] = b;
                    n += 1;
                    if b == b'\n' {
                        break;
                    }
                }
                None => break,
            }
        }
        n
    }
    fn cargar_segmentos(&mut self, datos: &[u8]) -> Result<u32, u32> {
        if datos.starts_with(&[0, 0, 3, 0xF3]) {
            self.cargado.push(datos.to_vec());
            Ok(0x1234)
        } else {
            Err(caja_dos::errores::ERROR_BAD_HUNK)
        }
    }
    fn descargar_segmentos(&mut self, s: u32) {
        self.descargado.push(s);
    }
    fn senales(&mut self, m: u32) -> u32 {
        let v = self.senales & m;
        self.senales &= !m;
        v
    }
}

impl CajaPrueba {
    pub fn nueva() -> CajaPrueba {
        CajaPrueba {
            mem: MemoriaVec::nueva(TAM_MEMORIA),
            siguiente: PRIMERA_LIBRE,
            vivos: BTreeMap::new(),
            tarea: 0,
            salida: Vec::new(),
            entrada: VecDeque::new(),
            hora: 1_700_000_000,
            senales: 0,
            cargado: Vec::new(),
            descargado: Vec::new(),
            reservas_restantes: None,
        }
    }
}

/// Un servicio de archivos que mira lo que le llega: nombres limpios y asas que él dio. Lo que no, se apunta.
pub struct Espia {
    pub fs: MemFs,
    dadas: BTreeSet<Asa>,
    pub faltas: Vec<String>,
}

impl Espia {
    pub fn nuevo(fs: MemFs) -> Espia {
        Espia { fs, dadas: (0..16).collect(), faltas: Vec::new() }
    }
    fn asa(&mut self, a: Asa, orden: &str) {
        if !self.dadas.contains(&a) {
            self.faltas.push(format!("{orden}: asa {a} que el servicio no dio (o ya cerrada)"));
        }
    }
    fn camino(&mut self, c: &str, orden: &str) {
        if c.is_empty() {
            return;
        }
        if c.len() > LARGO_CAMINO {
            self.faltas.push(format!("{orden}: camino de {} bytes", c.len()));
        }
        for p in c.split('/') {
            if p.is_empty() || p == "." || p == ".." || p.contains(':') || p.contains('\0') {
                self.faltas.push(format!("{orden}: camino sucio {c:?}"));
            }
        }
    }
    fn nombre(&mut self, n: &str, orden: &str) {
        if n.is_empty() || n == "." || n == ".." || n.contains(['/', ':', '\0']) {
            self.faltas.push(format!("{orden}: nombre sucio {n:?}"));
        }
    }
    fn dio(&mut self, r: &Result<(Asa, Datos), ErrorFs>) {
        if let Ok((a, _)) = r {
            self.dadas.insert(*a);
        }
    }
    pub fn abiertas(&self) -> usize {
        self.fs.asas_abiertas()
    }
}

impl SistemaArchivos for Espia {
    fn buscar(&mut self, asa: Asa, camino: &str) -> Result<(Asa, Datos), ErrorFs> {
        self.asa(asa, "buscar");
        self.camino(camino, "buscar");
        let r = self.fs.buscar(asa, camino);
        self.dio(&r);
        r
    }
    fn mirar(&mut self, asa: Asa, camino: &str) -> Result<Datos, ErrorFs> {
        self.asa(asa, "mirar");
        self.camino(camino, "mirar");
        self.fs.mirar(asa, camino)
    }
    fn leer(&mut self, asa: Asa, desde: u64, buf: &mut [u8]) -> Result<usize, ErrorFs> {
        self.asa(asa, "leer");
        self.fs.leer(asa, desde, buf)
    }
    fn escribir(&mut self, asa: Asa, desde: u64, datos: &[u8]) -> Result<usize, ErrorFs> {
        self.asa(asa, "escribir");
        self.fs.escribir(asa, desde, datos)
    }
    fn listar(&mut self, asa: Asa, desde: u64) -> Result<Pagina, ErrorFs> {
        self.asa(asa, "listar");
        self.fs.listar(asa, desde)
    }
    fn datos(&mut self, asa: Asa) -> Result<Datos, ErrorFs> {
        self.asa(asa, "datos");
        self.fs.datos(asa)
    }
    fn cerrar(&mut self, asa: Asa) -> Result<(), ErrorFs> {
        self.asa(asa, "cerrar");
        if asa < 16 {
            self.faltas.push(format!("cerrar la raíz {asa}"));
        }
        self.dadas.remove(&asa);
        self.fs.cerrar(asa)
    }
    fn crear(&mut self, asa: Asa, nombre: &str, cajon: bool) -> Result<(Asa, Datos), ErrorFs> {
        self.asa(asa, "crear");
        self.nombre(nombre, "crear");
        let r = self.fs.crear(asa, nombre, cajon);
        self.dio(&r);
        r
    }
    fn borrar(&mut self, asa: Asa, nombre: &str) -> Result<(), ErrorFs> {
        self.asa(asa, "borrar");
        self.nombre(nombre, "borrar");
        self.fs.borrar(asa, nombre)
    }
    fn renombrar(&mut self, asa: Asa, viejo: &str, asa_nueva: Asa, nuevo: &str) -> Result<(), ErrorFs> {
        self.asa(asa, "renombrar");
        self.asa(asa_nueva, "renombrar");
        self.nombre(viejo, "renombrar");
        self.nombre(nuevo, "renombrar");
        self.fs.renombrar(asa, viejo, asa_nueva, nuevo)
    }
    fn cambiar_tamano(&mut self, asa: Asa, tamano: u64) -> Result<u64, ErrorFs> {
        self.asa(asa, "tamaño");
        self.fs.cambiar_tamano(asa, tamano)
    }
    fn vaciar(&mut self) -> Result<(), ErrorFs> {
        self.fs.vaciar()
    }
    fn info(&mut self, asa: Asa) -> Result<InfoVolumen, ErrorFs> {
        self.asa(asa, "info");
        self.fs.info(asa)
    }
    fn comentario(&mut self, asa: Asa) -> Result<String, ErrorFs> {
        self.asa(asa, "comentario");
        self.fs.comentario(asa)
    }
    fn comentar(&mut self, asa: Asa, c: &str) -> Result<(), ErrorFs> {
        self.asa(asa, "comentar");
        self.fs.comentar(asa, c)
    }
    fn proteger(&mut self, asa: Asa, amiga: u16, modo: u16) -> Result<(), ErrorFs> {
        self.asa(asa, "proteger");
        self.fs.proteger(asa, amiga, modo)
    }
    fn fechar(&mut self, asa: Asa, fecha: u64) -> Result<(), ErrorFs> {
        self.asa(asa, "fechar");
        self.fs.fechar(asa, fecha)
    }
}

/// La caja, dos y un servicio con tres volúmenes: SYS (el de arranque), Work y RAM; y uno de solo lectura, CD0.
pub struct Banco {
    pub dos: Dos<Espia>,
    pub caja: CajaPrueba,
    pub sys: Asa,
    pub work: Asa,
    pub ram: Asa,
    pub cd: Asa,
    pub proceso: u32,
}

pub use caja_dos::estructuras::DOSTRUE;

impl Banco {
    pub fn nuevo() -> Banco {
        let mut fs = MemFs::nuevo();
        let sys = fs.nuevo_volumen(false, 1 << 30);
        let work = fs.nuevo_volumen(false, 1 << 30);
        let ram = fs.nuevo_volumen(false, 1 << 20);
        let cd = fs.nuevo_volumen(true, 1 << 30);
        fs.poner(sys, "S/Startup-Sequence", b"; arranque\n");
        fs.poner_cajon(sys, "Libs");
        fs.poner_cajon(sys, "C");
        fs.poner(sys, "C/Dir", &[0, 0, 3, 0xF3, 1, 2, 3, 4]);
        fs.poner(cd, "Leeme", b"solo lectura");
        let mut caja = CajaPrueba::nueva();
        let proceso = caja.reservar(process::TAMANO);
        caja.mem.escribir8(proceso + process::LN_TYPE, process::NT_PROCESS);
        caja.tarea = proceso;
        let mut dos = Dos::nuevo(Espia::nuevo(fs));
        dos.montar(&mut caja, "SYS", sys, false).unwrap();
        dos.montar(&mut caja, "Work", work, false).unwrap();
        dos.montar(&mut caja, "RAM", ram, false).unwrap();
        dos.montar(&mut caja, "CD0", cd, true).unwrap();
        let cons = dos.abrir_consola(&mut caja).unwrap();
        caja.mem.escribir32(proceso + process::PR_CIS, cons);
        caja.mem.escribir32(proceso + process::PR_COS, cons);
        Banco { dos, caja, sys, work, ram, cd, proceso }
    }

    pub fn fs(&mut self) -> &mut MemFs {
        &mut self.dos.fs.fs
    }

    /// Llama a la función `n` con D1, D2... = `args`. Devuelve D0.
    pub fn llamar(&mut self, n: u16, args: &[u32]) -> u32 {
        let mut r = Registros::default();
        for (i, a) in args.iter().enumerate() {
            r.d[i + 1] = *a;
        }
        let acc = self.dos.llamar(&mut self.caja, n, &mut r);
        assert_eq!(acc, Accion::Seguir, "función {n}");
        r.d[0]
    }

    /// Una cadena C en la memoria de la caja (Latin-1: cada carácter es un byte).
    pub fn cad(&mut self, s: &str) -> u32 {
        let b: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
        self.bytes(&b)
    }

    pub fn bytes(&mut self, b: &[u8]) -> u32 {
        let d = self.caja.reservar(b.len() as u32 + 1);
        self.caja.mem.trozo_mut(d, b.len() as u32).unwrap().copy_from_slice(b);
        d
    }

    pub fn hueco(&mut self, n: u32) -> u32 {
        self.caja.reservar(n)
    }

    pub fn leer(&self, d: u32, n: u32) -> Vec<u8> {
        self.caja.mem.trozo(d, n).unwrap().to_vec()
    }

    pub fn l32(&self, d: u32) -> u32 {
        self.caja.mem.leer32(d).unwrap()
    }

    pub fn cadena_en(&self, d: u32) -> String {
        let mut s = String::new();
        let mut i = d;
        loop {
            let b = self.caja.mem.leer8(i).unwrap();
            if b == 0 {
                return s;
            }
            s.push(b as char);
            i += 1;
        }
    }

    pub fn io_err(&self) -> u32 {
        self.l32(self.proceso + process::PR_RESULT2)
    }

    pub fn poner_io_err(&mut self, v: u32) {
        self.caja.mem.escribir32(self.proceso + process::PR_RESULT2, v);
    }

    pub fn open(&mut self, ruta: &str, modo: i32) -> u32 {
        let n = self.cad(ruta);
        self.llamar(lvo::OPEN, &[n, modo as u32])
    }

    pub fn lock(&mut self, ruta: &str, modo: i32) -> u32 {
        let n = self.cad(ruta);
        self.llamar(lvo::LOCK, &[n, modo as u32])
    }

    pub fn write(&mut self, fh: u32, datos: &[u8]) -> u32 {
        let b = self.bytes(datos);
        self.llamar(lvo::WRITE, &[fh, b, datos.len() as u32])
    }

    pub fn read(&mut self, fh: u32, n: u32) -> (u32, Vec<u8>) {
        let b = self.hueco(n.max(1));
        let r = self.llamar(lvo::READ, &[fh, b, n]);
        let k = if (r as i32) > 0 { r } else { 0 };
        (r, self.leer(b, k))
    }

    pub fn close(&mut self, fh: u32) -> u32 {
        self.llamar(lvo::CLOSE, &[fh])
    }

    pub fn unlock(&mut self, l: u32) {
        self.llamar(lvo::UN_LOCK, &[l]);
    }

    pub fn fib(&mut self) -> u32 {
        self.hueco(fib::TAMANO)
    }

    pub fn nombre_fib(&self, f: u32) -> String {
        self.cadena_en(f + fib::FIB_FILENAME)
    }

    pub fn examine(&mut self, l: u32) -> (u32, u32) {
        let f = self.fib();
        let r = self.llamar(lvo::EXAMINE, &[l, f]);
        (r, f)
    }

    /// Los nombres de un cajón con Examine y ExNext.
    pub fn listar(&mut self, ruta: &str) -> Vec<String> {
        let l = self.lock(ruta, SHARED_LOCK);
        assert_ne!(l, 0, "Lock({ruta})");
        let (r, f) = self.examine(l);
        assert_eq!(r, DOSTRUE);
        let mut v = Vec::new();
        while self.llamar(lvo::EX_NEXT, &[l, f]) == DOSTRUE {
            v.push(self.nombre_fib(f));
            assert!(v.len() < 10_000);
        }
        assert_eq!(self.io_err(), caja_dos::errores::ERROR_NO_MORE_ENTRIES);
        self.unlock(l);
        v
    }

    pub fn name_from_lock(&mut self, l: u32) -> String {
        let b = self.hueco(256);
        assert_eq!(self.llamar(lvo::NAME_FROM_LOCK, &[l, b, 256]), DOSTRUE, "NameFromLock");
        self.cadena_en(b)
    }

    pub fn avisos(&mut self) -> Vec<Aviso> {
        self.dos.tomar_avisos()
    }

    /// Que el servicio no haya visto nada sucio.
    pub fn sin_faltas(&self) {
        assert!(self.dos.fs.faltas.is_empty(), "el servicio vio: {:?}", self.dos.fs.faltas);
    }

    /// Que no quede nada abierto (además de lo que se diga).
    pub fn limpio(&self, cerrojos: usize, ficheros: usize) {
        self.sin_faltas();
        assert_eq!(self.dos.cerrojos_abiertos(), cerrojos, "cerrojos abiertos");
        assert_eq!(self.dos.ficheros_abiertos(), ficheros, "ficheros abiertos");
        assert_eq!(self.dos.fs.abiertas(), cerrojos + ficheros - 1, "asas del servicio (sin la consola)");
    }
}
