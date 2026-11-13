//! La guardia de los ficheros (S28): volúmenes de solo lectura volumen a volumen (decisión 8), y ejecutables,
//! iconos, guiones de arranque y bloques de arranque de imágenes que no se cambian. Con los ataques de los virus del
//! Amiga por dos.library, y al azar.

mod comun;
use comun::*;

use caja_dos::errores::*;
use caja_dos::estructuras::*;
use caja_dos::guardia::{Clase, Guardia, clase_por_nombre};
use caja_dos::*;

const HUNK: &[u8] = &[0, 0, 3, 0xF3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4];

struct Caso {
    dos: Dos<Guardia<Espia>>,
    caja: CajaPrueba,
    sys: Asa,
    trabajo: Asa,
    proceso: u32,
}

fn adf() -> Vec<u8> {
    let mut v = vec![0u8; 11 * 512];
    v[..4].copy_from_slice(b"DOS\x01");
    v
}

impl Caso {
    fn nuevo() -> Caso {
        let mut fs = MemFs::nuevo();
        // Los dos volúmenes se pueden escribir en el servicio: lo de solo lectura lo pone dos (la guardia).
        let sys = fs.nuevo_volumen(false, 1 << 30);
        let trabajo = fs.nuevo_volumen(false, 1 << 30);
        fs.poner(sys, "LEEME.txt", b"DashOS lee RedoxFS\n");
        fs.poner(sys, "Imagenes/disquete.adf", &adf());
        fs.poner(sys, "C/Dir", HUNK);
        fs.poner(trabajo, "C/saludo", HUNK);
        fs.poner(trabajo, "C/saludo.info", &[0xE3, 0x10, 0, 1, 0, 0, 0, 0]);
        fs.poner(trabajo, "S/Startup-Sequence", b"saludo\n");
        fs.poner(trabajo, "Discos/disco.adf", &adf());
        fs.poner(trabajo, "datos.txt", b"datos\n");
        fs.poner(trabajo, "texto", b"no es un ejecutable");
        let mut caja = CajaPrueba::nueva();
        let proceso = caja.reservar(process::TAMANO);
        caja.mem.escribir8(proceso + process::LN_TYPE, process::NT_PROCESS);
        caja.tarea = proceso;
        let mut dos = Dos::nuevo(Guardia::nueva(Espia::nuevo(fs)));
        dos.montar(&mut caja, "SYS", sys, true).unwrap();
        dos.montar(&mut caja, "Caja", trabajo, false).unwrap();
        Caso { dos, caja, sys, trabajo, proceso }
    }

    fn mem(&self) -> &MemFs {
        &self.dos.fs.dentro.fs
    }

    fn llamar(&mut self, n: u16, args: &[u32]) -> u32 {
        let mut r = Registros::default();
        for (i, a) in args.iter().enumerate() {
            r.d[i + 1] = *a;
        }
        let acc = self.dos.llamar(&mut self.caja, n, &mut r);
        assert_eq!(acc, Accion::Seguir, "función {n}");
        r.d[0]
    }

    fn cad(&mut self, s: &str) -> u32 {
        let b: Vec<u8> = s.bytes().collect();
        let d = self.caja.reservar(b.len() as u32 + 1);
        self.caja.mem.trozo_mut(d, b.len() as u32).unwrap().copy_from_slice(&b);
        d
    }

    fn io_err(&self) -> u32 {
        self.caja.mem.leer32(self.proceso + process::PR_RESULT2).unwrap()
    }

    fn open(&mut self, ruta: &str, modo: i32) -> u32 {
        let n = self.cad(ruta);
        self.llamar(lvo::OPEN, &[n, modo as u32])
    }

    fn write(&mut self, fh: u32, datos: &[u8]) -> u32 {
        let b = self.caja.reservar(datos.len() as u32 + 1);
        self.caja.mem.trozo_mut(b, datos.len() as u32).unwrap().copy_from_slice(datos);
        self.llamar(lvo::WRITE, &[fh, b, datos.len() as u32])
    }

    fn seek(&mut self, fh: u32, pos: u32) {
        self.llamar(lvo::SEEK, &[fh, pos, OFFSET_BEGINNING as u32]);
    }

    fn close(&mut self, fh: u32) {
        self.llamar(lvo::CLOSE, &[fh]);
    }

    fn borrar(&mut self, ruta: &str) -> u32 {
        let n = self.cad(ruta);
        self.llamar(lvo::DELETE_FILE, &[n])
    }

    fn renombrar(&mut self, a: &str, b: &str) -> u32 {
        let (x, y) = (self.cad(a), self.cad(b));
        self.llamar(lvo::RENAME, &[x, y])
    }

    /// Abre para escribir (como un virus de enlace: OLDFILE) y escribe al final: lo que devuelve Write y el IoErr.
    fn pegar_al_final(&mut self, ruta: &str) -> (u32, u32) {
        let f = self.open(ruta, MODE_OLDFILE);
        assert_ne!(f, 0, "{ruta} se abre para leer");
        self.llamar(lvo::SEEK, &[f, 0, OFFSET_END as u32]);
        let w = self.write(f, b"VIRUS");
        let e = self.io_err();
        self.close(f);
        (w, e)
    }

    /// Lo que debe seguir igual.
    fn intacto(&self) {
        let m = self.mem();
        assert_eq!(m.contenido(self.sys, "LEEME.txt").unwrap(), b"DashOS lee RedoxFS\n");
        assert_eq!(m.contenido(self.sys, "Imagenes/disquete.adf").unwrap(), adf());
        assert_eq!(m.contenido(self.sys, "C/Dir").unwrap(), HUNK);
        assert_eq!(m.contenido(self.trabajo, "C/saludo").unwrap(), HUNK);
        assert_eq!(m.contenido(self.trabajo, "C/saludo.info").unwrap(), [0xE3, 0x10, 0, 1, 0, 0, 0, 0]);
        assert_eq!(m.contenido(self.trabajo, "S/Startup-Sequence").unwrap(), b"saludo\n");
        assert_eq!(m.contenido(self.trabajo, "Discos/disco.adf").unwrap()[..1024], adf()[..1024]);
        assert!(!m.existe(self.trabajo, "S/User-Startup"));
    }
}

#[test]
fn clases_por_nombre() {
    assert_eq!(clase_por_nombre("Startup-Sequence"), Clase::Arranque);
    assert_eq!(clase_por_nombre("USER-STARTUP"), Clase::Arranque);
    assert_eq!(clase_por_nombre("Disk-Validator"), Clase::Arranque);
    assert_eq!(clase_por_nombre("Disk.info"), Clase::Icono);
    assert_eq!(clase_por_nombre("juego.ADF"), Clase::Imagen(1024));
    assert_eq!(clase_por_nombre("disco.hdf"), Clase::Imagen(8192));
    assert_eq!(clase_por_nombre("saludo"), Clase::Normal);
    assert_eq!(clase_por_nombre("info"), Clase::Normal);
}

#[test]
fn volumen_de_solo_lectura() {
    let mut c = Caso::nuevo();
    // Leer, sí.
    let f = c.open("SYS:LEEME.txt", MODE_OLDFILE);
    assert_ne!(f, 0);
    // Escribir encima, no: ERROR_DISK_WRITE_PROTECTED, aunque el servicio lo dejaría.
    assert_eq!(c.write(f, b"x"), u32::MAX);
    assert_eq!(c.io_err(), ERROR_DISK_WRITE_PROTECTED);
    c.close(f);
    assert_eq!(c.open("SYS:LEEME.txt", MODE_NEWFILE), 0);
    assert_eq!(c.io_err(), ERROR_DISK_WRITE_PROTECTED);
    assert_eq!(c.open("SYS:nuevo", MODE_NEWFILE), 0);
    assert_eq!(c.io_err(), ERROR_DISK_WRITE_PROTECTED);
    assert_eq!(c.borrar("SYS:LEEME.txt"), DOSFALSE);
    assert_eq!(c.io_err(), ERROR_DISK_WRITE_PROTECTED);
    assert_eq!(c.renombrar("SYS:LEEME.txt", "SYS:otro"), DOSFALSE);
    assert_eq!(c.io_err(), ERROR_DISK_WRITE_PROTECTED);
    let (n, d) = (c.cad("SYS:LEEME.txt"), c.cad("virus"));
    assert_eq!(c.llamar(lvo::SET_COMMENT, &[n, d]), DOSFALSE);
    assert_eq!(c.llamar(lvo::SET_PROTECTION, &[n, 0]), DOSFALSE);
    let n = c.cad("SYS:Nuevo");
    assert_eq!(c.llamar(lvo::CREATE_DIR, &[n]), 0);
    assert_eq!(c.io_err(), ERROR_DISK_WRITE_PROTECTED);
    // El bloque de arranque del ADF de SYS:, tampoco.
    let f = c.open("SYS:Imagenes/disquete.adf", MODE_OLDFILE);
    assert_eq!(c.write(f, &[0xAA; 1024]), u32::MAX);
    assert_eq!(c.io_err(), ERROR_DISK_WRITE_PROTECTED);
    c.close(f);
    c.intacto();
    let neg = c.dos.fs.tomar_negados();
    assert!(neg.len() >= 8 && neg.iter().all(|n| n.motivo == "volumen de solo lectura"), "{neg:?}");
}

#[test]
fn virus_de_enlace() {
    let mut c = Caso::nuevo();
    // Pegarse al final, vaciarlo y reescribirlo, renombrarlo para esconderlo, borrarlo: nada.
    assert_eq!(c.pegar_al_final("Caja:C/saludo"), (u32::MAX, ERROR_WRITE_PROTECTED));
    assert_eq!(c.open("Caja:C/saludo", MODE_NEWFILE), 0);
    assert_eq!(c.io_err(), ERROR_WRITE_PROTECTED);
    let f = c.open("Caja:C/saludo", MODE_READWRITE);
    assert_eq!(c.write(f, HUNK), u32::MAX);
    assert_eq!(c.llamar(lvo::SET_FILE_SIZE, &[f, 0, OFFSET_BEGINNING as u32]), u32::MAX);
    c.close(f);
    assert_eq!(c.renombrar("Caja:C/saludo", "Caja:C/.saludo"), DOSFALSE);
    assert_eq!(c.io_err(), ERROR_WRITE_PROTECTED);
    assert_eq!(c.borrar("Caja:C/saludo"), DOSFALSE);
    assert_eq!(c.io_err(), ERROR_DELETE_PROTECTED);
    let n = c.cad("Caja:C/saludo");
    assert_eq!(c.llamar(lvo::SET_PROTECTION, &[n, 0]), DOSFALSE);
    // En SYS:, además, el volumen.
    assert_eq!(c.pegar_al_final("SYS:C/Dir"), (u32::MAX, ERROR_DISK_WRITE_PROTECTED));
    c.intacto();
    // Un ejecutable nuevo sí se crea y se escribe (un compilador), hasta que se cierra.
    let f = c.open("Caja:C/nuevo", MODE_NEWFILE);
    assert_eq!(c.write(f, HUNK), HUNK.len() as u32);
    assert_eq!(c.write(f, b"mas"), 3);
    c.close(f);
    assert_eq!(c.pegar_al_final("Caja:C/nuevo"), (u32::MAX, ERROR_WRITE_PROTECTED));
    // Un fichero de datos sí se cambia, aunque se llame como un programa.
    assert_eq!(c.pegar_al_final("Caja:texto").0, 5);
    assert_eq!(c.mem().contenido(c.trabajo, "texto").unwrap(), b"no es un ejecutableVIRUS");
}

#[test]
fn guiones_iconos_e_imagenes() {
    let mut c = Caso::nuevo();
    assert_eq!(c.pegar_al_final("Caja:S/Startup-Sequence"), (u32::MAX, ERROR_WRITE_PROTECTED));
    // Ni crear un guion de arranque nuevo, ni ponerle ese nombre a otro fichero.
    assert_eq!(c.open("Caja:S/User-Startup", MODE_NEWFILE), 0);
    assert_eq!(c.io_err(), ERROR_WRITE_PROTECTED);
    assert_eq!(c.renombrar("Caja:datos.txt", "Caja:S/User-Startup"), DOSFALSE);
    assert_eq!(c.renombrar("Caja:S/Startup-Sequence", "Caja:S/viejo"), DOSFALSE);
    assert_eq!(c.borrar("Caja:S/Startup-Sequence"), DOSFALSE);
    assert_eq!(c.open("Caja:L/Disk-Validator", MODE_NEWFILE), 0);
    // El icono que ya existe no se cambia; uno nuevo, sí.
    assert_eq!(c.pegar_al_final("Caja:C/saludo.info"), (u32::MAX, ERROR_WRITE_PROTECTED));
    let f = c.open("Caja:datos.txt.info", MODE_NEWFILE);
    assert_eq!(c.write(f, &[0xE3, 0x10]), 2);
    c.close(f);
    // La imagen: el bloque de arranque no; el resto, sí; recortarla por debajo, no.
    let f = c.open("Caja:Discos/disco.adf", MODE_OLDFILE);
    assert_eq!(c.write(f, &[0xAA; 1024]), u32::MAX);
    assert_eq!(c.io_err(), ERROR_WRITE_PROTECTED);
    c.seek(f, 1000);
    assert_eq!(c.write(f, &[0xAA; 100]), u32::MAX);
    c.seek(f, 2048);
    assert_eq!(c.write(f, &[0xBB; 512]), 512);
    assert_eq!(c.llamar(lvo::SET_FILE_SIZE, &[f, 512, OFFSET_BEGINNING as u32]), u32::MAX);
    c.close(f);
    assert_eq!(c.borrar("Caja:Discos/disco.adf"), DOSFALSE);
    assert_eq!(c.mem().contenido(c.trabajo, "Discos/disco.adf").unwrap()[2048..2560], [0xBB; 512]);
    c.intacto();
    // Los datos propios se borran: para eso haría falta la papelera (S28).
    assert_eq!(c.borrar("Caja:datos.txt"), DOSTRUE);
    let neg = c.dos.fs.tomar_negados();
    assert!(neg.iter().any(|n| n.motivo == "guion de arranque" && n.orden == "crear"), "{neg:?}");
    assert!(neg.iter().any(|n| n.motivo == "icono"), "{neg:?}");
    assert!(neg.iter().any(|n| n.motivo.starts_with("bloque de arranque")), "{neg:?}");
}

struct Azar(u64);
impl Azar {
    fn sig(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn de<T: Copy>(&mut self, v: &[T]) -> T {
        v[(self.sig() % v.len() as u64) as usize]
    }
}

/// Al azar: abrir de todas las formas, escribir en cualquier sitio, recortar, borrar, renombrar y proteger lo
/// protegido (y lo demás). Lo protegido sigue igual, la guardia no pierde la cuenta de las asas y el servicio solo ve
/// asas que dio.
#[test]
fn hostil_al_azar() {
    const RUTAS: &[&str] = &[
        "SYS:LEEME.txt",
        "SYS:Imagenes/disquete.adf",
        "SYS:C/Dir",
        "Caja:C/saludo",
        "Caja:C/SALUDO",
        "Caja:C/saludo.info",
        "Caja:S/Startup-Sequence",
        "Caja:s/startup-sequence",
        "Caja:S/User-Startup",
        "Caja:Discos/disco.adf",
        "Caja:datos.txt",
        "Caja:nuevo",
        "Caja:C/otro.info",
        "Caja:L/Disk-Validator",
        "Caja:",
        "SYS:",
    ];
    for semilla in 1..40u64 {
        let mut c = Caso::nuevo();
        let mut az = Azar(semilla.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut abiertos = Vec::new();
        for _ in 0..400 {
            match az.sig() % 9 {
                0 => {
                    let m = az.de(&[MODE_OLDFILE, MODE_NEWFILE, MODE_READWRITE]);
                    let r = az.de(RUTAS);
                    let f = c.open(r, m);
                    if f != 0 {
                        abiertos.push(f);
                    }
                }
                1 | 2 if !abiertos.is_empty() => {
                    let f = az.de(&abiertos);
                    c.seek(f, (az.sig() % 3000) as u32);
                    let n = (az.sig() % 1500) as usize;
                    c.write(f, &vec![0x66; n]);
                }
                3 if !abiertos.is_empty() => {
                    let f = az.de(&abiertos);
                    c.llamar(lvo::SET_FILE_SIZE, &[f, (az.sig() % 2000) as u32, OFFSET_BEGINNING as u32]);
                }
                4 if !abiertos.is_empty() => {
                    let i = (az.sig() % abiertos.len() as u64) as usize;
                    let f = abiertos.swap_remove(i);
                    c.close(f);
                }
                5 => {
                    let r = az.de(RUTAS);
                    c.borrar(r);
                }
                6 => {
                    let (a, b) = (az.de(RUTAS), az.de(RUTAS));
                    c.renombrar(a, b);
                }
                7 => {
                    let n = c.cad(az.de(RUTAS));
                    c.llamar(lvo::SET_PROTECTION, &[n, az.sig() as u32]);
                }
                _ => {
                    let (n, t) = (c.cad(az.de(RUTAS)), c.cad("comentario"));
                    c.llamar(lvo::SET_COMMENT, &[n, t]);
                }
            }
        }
        c.intacto();
        // Lo de SYS: no lo cambió nadie: ni un nombre nuevo.
        assert!(!c.mem().existe(c.sys, "nuevo"));
        let p = c.proceso;
        c.dos.acabar_tarea(&mut c.caja, p);
        assert_eq!(c.dos.fs.asas_seguidas(), 0, "semilla {semilla}");
        assert_eq!(c.dos.fs.dentro.abiertas(), 0, "semilla {semilla}");
        assert!(c.dos.fs.dentro.faltas.is_empty(), "{:?}", c.dos.fs.dentro.faltas);
        assert!(c.dos.fs.total_negados > 0);
    }
}

#[test]
fn tras_negar_se_sigue_leyendo() {
    let mut c = Caso::nuevo();
    assert_eq!(c.open("SYS:/../../fuera", MODE_OLDFILE), 0);
    assert_eq!(c.open("SYS:Nuevo.txt", MODE_NEWFILE), 0);
    let f = c.open("SYS:LEEME.txt", MODE_OLDFILE);
    assert_ne!(f, 0, "IoErr {}", c.io_err());
    c.close(f);
    let l = { let n = c.cad("SYS:"); c.llamar(lvo::LOCK, &[n, (-2i32) as u32]) };
    assert_ne!(l, 0);
    let p = c.proceso;
    c.dos.acabar_tarea(&mut c.caja, p);
    let f = c.open("SYS:LEEME.txt", MODE_OLDFILE);
    assert_ne!(f, 0, "IoErr {}", c.io_err());
}
