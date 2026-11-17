//! Las pruebas del instalador en el PC: `rustc --edition 2024 --test usuario/src/instalador/pruebas.rs`. Discos en
//! memoria (dispersos: un disco de 64 GB solo guarda lo escrito), tablas rotas a propósito, cortes de luz en cada
//! escritura, FAT con nombres largos y planes escritos a mano que intentan pisar algo.
//!
//! Con `DASHOS_DISCO=<imagen cruda>` (por ejemplo una capa de qcow2 exportada con qemu-storage-daemon), la prueba
//! `disco_de_verdad` lee ese disco y escribe en la salida lo que ve (no escribe nada).

extern crate alloc;

#[path = "aplicar.rs"]
#[allow(dead_code)]
mod aplicar;
#[path = "disco.rs"]
#[allow(dead_code)]
mod disco;
#[path = "fat.rs"]
#[allow(dead_code)]
mod fat;
#[path = "gpt.rs"]
#[allow(dead_code)]
mod gpt;
#[path = "nvram.rs"]
#[allow(dead_code)]
mod nvram;
#[path = "plan.rs"]
#[allow(dead_code)]
mod plan;
#[path = "reconocer.rs"]
#[allow(dead_code)]
mod reconocer;

use disco::Disco;
use gpt::{Copias, Nueva, tipos};
use plan::{DiscoVisto, Esp, Modo, Pedido};
use std::collections::BTreeMap;

/// Un disco disperso en memoria.
struct Disperso {
    sectores: u64,
    sector: usize,
    datos: BTreeMap<u64, Vec<u8>>,
    corte: Option<usize>,
    escrituras: usize,
}

impl Disperso {
    fn nuevo(bytes: u64) -> Self {
        Disperso { sectores: bytes / 512, sector: 512, datos: BTreeMap::new(), corte: None, escrituras: 0 }
    }
}

impl Disco for Disperso {
    fn tam_sector(&self) -> usize {
        self.sector
    }
    fn sectores(&self) -> u64 {
        self.sectores
    }
    fn leer(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
        let n = (buf.len() / self.sector) as u64;
        if buf.len() % self.sector != 0 || lba + n > self.sectores {
            return Err(());
        }
        for i in 0..n {
            let b = &mut buf[i as usize * self.sector..][..self.sector];
            match self.datos.get(&(lba + i)) {
                Some(s) => b.copy_from_slice(s),
                None => b.fill(0),
            }
        }
        Ok(())
    }
    fn escribir(&mut self, lba: u64, buf: &[u8]) -> Result<(), ()> {
        let n = (buf.len() / self.sector) as u64;
        if buf.len() % self.sector != 0 || lba + n > self.sectores {
            return Err(());
        }
        // El corte cuenta sectores: así cae también en mitad de una escritura de varios.
        for i in 0..n {
            if let Some(c) = self.corte.as_mut() {
                if *c == 0 {
                    return Err(());
                }
                *c -= 1;
            }
            self.escrituras += 1;
            self.datos.insert(lba + i, buf[i as usize * self.sector..][..self.sector].to_vec());
        }
        Ok(())
    }
    fn vaciar(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

const GB: u64 = 1 << 30;
const MIB: u64 = 2048;

fn guid(n: u8) -> [u8; 16] {
    let mut g = [n; 16];
    g[7] = 0x40 | (n & 0xF);
    g
}

/// Un disco con GPT y las particiones dadas (tipo, desde, hasta en sectores).
fn disco_con(bytes: u64, ps: &[([u8; 16], u64, u64)]) -> Disperso {
    let mut d = Disperso::nuevo(bytes);
    let t = gpt::tabla_vacia(d.sectores, 512, guid(0xD1));
    let nuevas: Vec<Nueva> = ps.iter().enumerate().map(|(k, p)| Nueva { tipo: p.0, guid: guid(k as u8 + 1), desde: p.1, hasta: p.2, nombre: "prueba" }).collect();
    let lista = gpt::lista_nueva(&t, &nuevas, false).unwrap();
    d.escribir(0, &gpt::mbr_protector(d.sectores, 512)).unwrap();
    gpt::escribir_gpt(&mut d, &t, &lista).unwrap();
    d
}

/// Lo que ve el instalador de un disco.
fn ver(d: &mut Disperso, nombre: &str) -> DiscoVisto {
    let tabla = gpt::leer(d).unwrap();
    let contenidos = tabla.particiones.iter().map(|p| reconocer::reconocer(d, p.desde, p.hasta)).collect();
    DiscoVisto { nombre: nombre.into(), modelo: "de prueba".into(), sectores: d.sectores, sector: 512, tabla, contenidos, de_arranque: false }
}

fn guids() -> impl FnMut() -> [u8; 16] {
    let mut n = 0x80u8;
    move || {
        n += 1;
        guid(n)
    }
}

#[test]
fn gpt_vacia_y_huecos() {
    let mut d = disco_con(64 * GB, &[(tipos::ESP, MIB, 201 * MIB), (tipos::MS_DATOS, 217 * MIB, 40 * 1024 * MIB)]);
    let t = gpt::leer(&mut d).unwrap();
    assert_eq!(t.copias, Copias::Iguales);
    assert!(t.escribible());
    assert_eq!(t.particiones.len(), 2);
    let h = t.huecos(MIB);
    assert_eq!(h[0], (201 * MIB, 217 * MIB));
    assert_eq!(h[1].0, 40 * 1024 * MIB);
    assert!(h[1].1 > 63 * 1024 * MIB);
}

#[test]
fn respaldo_roto_o_distinto_no_se_escribe() {
    let mut d = disco_con(64 * GB, &[(tipos::MS_DATOS, MIB, 1000 * MIB)]);
    let ultimo = d.sectores - 1;
    // Un byte de la cabecera del final.
    let mut s = vec![0u8; 512];
    d.leer(ultimo, &mut s).unwrap();
    s[100] ^= 1;
    s[60] ^= 1;
    d.escribir(ultimo, &s).unwrap();
    let t = gpt::leer(&mut d).unwrap();
    assert_eq!(t.copias, Copias::RespaldoRoto);
    assert!(!t.escribible());
    let v = ver(&mut d, "sata0");
    assert!(plan::calcular(&v, Pedido::Hueco(None), &mut guids()).is_err());
    // Las dos buenas pero distintas (otra partición de Windows en la del final).
    let mut d = disco_con(64 * GB, &[(tipos::MS_DATOS, MIB, 1000 * MIB)]);
    let mut t = gpt::leer(&mut d).unwrap();
    let lista = gpt::lista_nueva(&t, &[Nueva { tipo: tipos::LINUX, guid: guid(9), desde: 2000 * MIB, hasta: 3000 * MIB, nombre: "x" }], false).unwrap();
    // Solo la copia del final: se corta tras ella.
    d.corte = Some(33);
    assert!(gpt::escribir_gpt(&mut d, &t, &lista).is_err());
    d.corte = None;
    t = gpt::leer(&mut d).unwrap();
    assert_eq!(t.copias, Copias::Distintas);
    assert!(!t.escribible());
    // La del principio rota: vale la del final, pero tampoco se escribe.
    let mut d = disco_con(64 * GB, &[(tipos::MS_DATOS, MIB, 1000 * MIB)]);
    d.escribir(1, &[0u8; 512]).unwrap();
    let t = gpt::leer(&mut d).unwrap();
    assert_eq!(t.copias, Copias::PrincipalRota);
    assert_eq!(t.particiones.len(), 1);
    assert!(!t.escribible());
}

#[test]
fn particiones_que_se_pisan_o_se_salen() {
    let mut d = disco_con(64 * GB, &[(tipos::MS_DATOS, MIB, 1000 * MIB)]);
    let t = gpt::leer(&mut d).unwrap();
    // A mano: una entrada que pisa a la otra (la lista_nueva no lo deja, así que se escribe cruda).
    let mut lista = t.lista.clone();
    lista[128..144].copy_from_slice(&tipos::LINUX);
    lista[144..160].copy_from_slice(&guid(7));
    lista[160..168].copy_from_slice(&(500 * MIB).to_le_bytes());
    lista[168..176].copy_from_slice(&(1500 * MIB).to_le_bytes());
    gpt::escribir_gpt(&mut d, &t, &lista).unwrap();
    let t = gpt::leer(&mut d).unwrap();
    assert!(!t.escribible());
    assert!(t.avisos.iter().any(|a| a.contains("pisan")));
}

#[test]
fn mbr_hibrido_y_sin_tabla() {
    let mut d = disco_con(64 * GB, &[(tipos::MS_DATOS, MIB, 1000 * MIB)]);
    let mut s0 = vec![0u8; 512];
    d.leer(0, &mut s0).unwrap();
    s0[446 + 16 + 4] = 0x07;
    s0[446 + 16 + 8..446 + 16 + 12].copy_from_slice(&(MIB as u32).to_le_bytes());
    s0[446 + 16 + 12..446 + 16 + 16].copy_from_slice(&(999 * MIB as u32).to_le_bytes());
    d.escribir(0, &s0).unwrap();
    let v = ver(&mut d, "sata0");
    assert_eq!(v.tabla.clase, gpt::Clase::GptHibrida);
    assert!(plan::calcular(&v, Pedido::Hueco(None), &mut guids()).unwrap_err().contains("híbrido"));
    let mut d = Disperso::nuevo(64 * GB);
    let v = ver(&mut d, "sata0");
    assert_eq!(v.tabla.clase, gpt::Clase::Ninguna);
    assert!(plan::calcular(&v, Pedido::Hueco(None), &mut guids()).is_err());
    let p = plan::calcular(&v, Pedido::DiscoEntero, &mut guids()).unwrap();
    assert_eq!(p.modo, Modo::DiscoEntero);
}

/// La escritura de la GPT, cortada en cada sector: la tabla queda como antes, o como después, o reconocida como una
/// instalación cortada (que se puede repetir); nunca rota sin más, y las particiones de antes siempre están.
#[test]
fn gpt_cortada_en_cada_sector() {
    let base = disco_con(64 * GB, &[(tipos::ESP, MIB, 201 * MIB), (tipos::MS_DATOS, 217 * MIB, 40 * 1024 * MIB)]);
    let antes = {
        let mut d = Disperso { datos: base.datos.clone(), ..Disperso::nuevo(64 * GB) };
        gpt::leer(&mut d).unwrap()
    };
    let lista = gpt::lista_nueva(&antes, &[Nueva { tipo: tipos::DASHOS, guid: guid(0x55), desde: 40 * 1024 * MIB, hasta: 60 * 1024 * MIB, nombre: "DashOS" }], false).unwrap();
    let mut total = 0;
    for corte in 0..200 {
        let mut d = Disperso { datos: base.datos.clone(), ..Disperso::nuevo(64 * GB) };
        d.corte = Some(corte);
        let r = gpt::escribir_gpt(&mut d, &antes, &lista);
        d.corte = None;
        let t = gpt::leer(&mut d).unwrap();
        assert!(t.escribible(), "corte {}: {:?} {:?}", corte, t.copias, t.avisos);
        assert!(t.particiones.iter().any(|p| p.tipo == tipos::MS_DATOS && p.desde == 217 * MIB), "corte {}", corte);
        assert!(t.particiones.iter().any(|p| p.tipo == tipos::ESP), "corte {}", corte);
        if r.is_ok() {
            total = corte;
            assert_eq!(t.copias, Copias::Iguales);
            break;
        }
        // Se puede repetir: el plan de hueco sale, sin la partición a medias.
        let v = DiscoVisto { nombre: "sata0".into(), modelo: String::new(), sectores: d.sectores, sector: 512, contenidos: vec![reconocer::Contenido::Vacia; t.particiones.len()], tabla: t, de_arranque: false };
        let v = DiscoVisto { contenidos: v.tabla.particiones.iter().map(|p| if p.tipo == tipos::ESP { reconocer::Contenido::Fat { bits: 32, etiqueta: String::new() } } else { reconocer::Contenido::Vacia }).collect(), ..v };
        let p = plan::calcular(&v, Pedido::Hueco(None), &mut guids()).unwrap_or_else(|e| panic!("corte {}: {}", corte, e));
        assert_eq!(p.dashos.0, 40 * 1024 * MIB);
    }
    assert!(total > 60, "la escritura entera son {} sectores", total);
}

#[test]
fn plan_hueco_esp_existente_y_texto() {
    let mut d = disco_con(64 * GB, &[(tipos::ESP, MIB, 201 * MIB), (tipos::MS_DATOS, 217 * MIB, 40 * 1024 * MIB)]);
    fat::formatear_fat32(&mut d, MIB, 201 * MIB, "SYSTEM", 0x1234).unwrap();
    let v = ver(&mut d, "sata0");
    assert!(matches!(v.contenidos[0], reconocer::Contenido::Fat { bits: 32, .. }));
    let p = plan::calcular(&v, Pedido::Hueco(None), &mut guids()).unwrap();
    assert_eq!(p.esp, Esp::Existente(1));
    assert_eq!(p.dashos.0, 40 * 1024 * MIB);
    let t = plan::texto(&p, &v);
    assert!(t.contains("Antes:") && t.contains("Después:"), "{}", t);
    let leido = plan::leer(&t).unwrap();
    assert_eq!(leido, p);
    plan::comprobar(&leido, &v).unwrap();
    // El pendrive del que arrancó: ni se ofrece.
    let mut v2 = v.clone();
    v2.de_arranque = true;
    assert!(plan::calcular(&v2, Pedido::Hueco(None), &mut guids()).unwrap_err().contains("pendrive"));
    assert!(plan::calcular(&v2, Pedido::DiscoEntero, &mut guids()).is_err());
    assert!(plan::comprobar(&leido, &v2).is_err());
    // El disco cambió desde el plan.
    let mut d3 = disco_con(64 * GB, &[(tipos::ESP, MIB, 201 * MIB), (tipos::MS_DATOS, 217 * MIB, 41 * 1024 * MIB)]);
    fat::formatear_fat32(&mut d3, MIB, 201 * MIB, "SYSTEM", 0x1234).unwrap();
    let v3 = ver(&mut d3, "sata0");
    assert!(plan::comprobar(&leido, &v3).is_err());
}

#[test]
fn planes_hostiles() {
    let mut d = disco_con(64 * GB, &[(tipos::ESP, MIB, 201 * MIB), (tipos::MS_DATOS, 217 * MIB, 40 * 1024 * MIB)]);
    fat::formatear_fat32(&mut d, MIB, 201 * MIB, "SYSTEM", 0x1234).unwrap();
    let v = ver(&mut d, "sata0");
    let p = plan::calcular(&v, Pedido::Hueco(None), &mut guids()).unwrap();
    let t = plan::texto(&p, &v);
    // Que DashOS empiece dentro de Windows.
    let malo = t.replace(&format!("dashos {} ", p.dashos.0), &format!("dashos {} ", 30 * 1024 * MIB));
    let pm = plan::leer(&malo).unwrap();
    assert!(plan::comprobar(&pm, &v).is_err());
    // Que pase a disco entero sin decirlo bien.
    assert!(plan::leer(&t.replace("modo hueco", "modo disco-entero")).is_err());
    // Repetido, sobrante, versión, basura.
    assert!(plan::leer(&format!("{}nvram si\n", t)).is_err());
    assert!(plan::leer(&t.replace("nvram si", "nvram si ya")).is_err());
    assert!(plan::leer(&t.replace("dashos-plan 1", "dashos-plan 2")).is_err());
    assert!(plan::leer("").is_err());
    assert!(plan::leer(&"x".repeat(100_000)).is_err());
    for i in 0..t.len() {
        if t.is_char_boundary(i) {
            let _ = plan::leer(&t[..i]);
        }
    }
    // Hueco demasiado pequeño.
    let mut d = disco_con(8 * GB, &[(tipos::ESP, MIB, 201 * MIB), (tipos::MS_DATOS, 217 * MIB, 6 * 1024 * MIB)]);
    fat::formatear_fat32(&mut d, MIB, 201 * MIB, "SYSTEM", 0x1234).unwrap();
    let v = ver(&mut d, "sata0");
    let e = plan::calcular(&v, Pedido::Hueco(None), &mut guids()).unwrap_err();
    assert!(e.contains("no hay sitio"), "{}", e);
}

#[test]
fn fat_poner_cargador_y_sustituir() {
    let mut d = disco_con(4 * GB, &[(tipos::ESP, MIB, 201 * MIB)]);
    fat::formatear_fat32(&mut d, MIB, 201 * MIB, "ESP", 7).unwrap();
    let mut f = fat::Fat::abrir(&mut d, MIB, 201 * MIB).unwrap();
    assert_eq!(f.bits, 32);
    let libres = f.libres();
    let efi = f.cajon(&mut d, 0, "EFI").unwrap();
    let ms = f.cajon(&mut d, efi, "Microsoft").unwrap();
    let ms_boot = f.cajon(&mut d, ms, "Boot").unwrap();
    f.poner_fichero(&mut d, ms_boot, "BOOTMGFW.EFI", b"de windows").unwrap();
    let dashos = f.cajon(&mut d, efi, "DashOS").unwrap();
    let grande: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7) as u8).collect();
    f.poner_fichero(&mut d, dashos, "DASHOS.EFI", &grande).unwrap();
    // Se vuelve a abrir de cero y se lee.
    let f2 = fat::Fat::abrir(&mut d, MIB, 201 * MIB).unwrap();
    let efi2 = f2.buscar(&mut d, 0, "efi").unwrap().unwrap();
    let ds = f2.buscar(&mut d, efi2.cluster, "dashos").unwrap().unwrap();
    assert_eq!(ds.nombre, "DashOS");
    let e = f2.buscar(&mut d, ds.cluster, "DASHOS.EFI").unwrap().unwrap();
    assert_eq!(f2.leer(&mut d, &e).unwrap(), grande);
    // Sustituir: lo de antes se libera, lo de Windows sigue.
    let mut f3 = f2;
    let antes = f3.libres();
    f3.poner_fichero(&mut d, ds.cluster, "DASHOS.EFI", b"corto").unwrap();
    assert!(f3.libres() > antes);
    let e = f3.buscar(&mut d, ds.cluster, "DASHOS.EFI").unwrap().unwrap();
    assert_eq!(f3.leer(&mut d, &e).unwrap(), b"corto");
    let w = f3.buscar(&mut d, ms_boot, "bootmgfw.efi").unwrap().unwrap();
    assert_eq!(f3.leer(&mut d, &w).unwrap(), b"de windows");
    assert!(f3.libres() < libres);
    // Muchos ficheros: el cajón crece.
    for i in 0..300 {
        f3.poner_fichero(&mut d, ds.cluster, &format!("F{}.TXT", i), format!("{}", i).as_bytes()).unwrap();
    }
    let f4 = fat::Fat::abrir(&mut d, MIB, 201 * MIB).unwrap();
    assert_eq!(f4.listar(&mut d, ds.cluster).unwrap().len(), 301);
}

/// Poner el cargador cortado en cada sector: lo de Windows sigue entero y la FAT se abre.
#[test]
fn fat_cortada_en_cada_sector() {
    let mut base = disco_con(4 * GB, &[(tipos::ESP, MIB, 201 * MIB)]);
    fat::formatear_fat32(&mut base, MIB, 201 * MIB, "ESP", 7).unwrap();
    let mut f = fat::Fat::abrir(&mut base, MIB, 201 * MIB).unwrap();
    let efi = f.cajon(&mut base, 0, "EFI").unwrap();
    let w = f.cajon(&mut base, efi, "BOOT").unwrap();
    f.poner_fichero(&mut base, w, "BOOTX64.EFI", &[0xAB; 70_000]).unwrap();
    let datos = vec![0x5Au8; 50_000];
    for corte in 0..400 {
        let mut d = Disperso { datos: base.datos.clone(), ..Disperso::nuevo(4 * GB) };
        d.corte = Some(corte);
        let mut f = fat::Fat::abrir(&mut d, MIB, 201 * MIB).unwrap();
        let r = f.cajon(&mut d, efi, "DashOS").and_then(|c| f.poner_fichero(&mut d, c, "DASHOS.EFI", &datos));
        d.corte = None;
        let f = fat::Fat::abrir(&mut d, MIB, 201 * MIB).unwrap();
        let b = f.buscar(&mut d, w, "BOOTX64.EFI").unwrap().unwrap();
        assert_eq!(f.leer(&mut d, &b).unwrap(), vec![0xAB; 70_000], "corte {}", corte);
        if let Some(ds) = f.buscar(&mut d, efi, "DashOS").unwrap() {
            if let Some(e) = f.buscar(&mut d, ds.cluster, "DASHOS.EFI").unwrap() {
                assert_eq!(f.leer(&mut d, &e).unwrap(), datos, "corte {}", corte);
            }
        }
        if r.is_ok() {
            return;
        }
    }
    panic!("no acabó en 400 sectores");
}

#[test]
fn nvram_opcion_y_orden() {
    let g = guid(3);
    let o = nvram::opcion(nvram::DESCRIPCION, 1, 2048, 206848, &g, nvram::CAMINO);
    let l = nvram::leer_opcion(&o).unwrap();
    assert_eq!(l.descripcion, "DashOS");
    assert_eq!(l.particion, Some(g));
    assert_eq!(l.camino.as_deref(), Some(nvram::CAMINO));
    let win = nvram::opcion("Windows Boot Manager", 1, 2048, 206848, &g, "\\EFI\\Microsoft\\Boot\\bootmgfw.efi");
    let (n, escribir, orden) = nvram::decidir(&[(0, win.clone()), (1, vec![1, 2, 3])], &[0, 1], &g, &o);
    assert_eq!((n, escribir), (2, true));
    assert_eq!(orden, vec![0, 1, 2]);
    // Ya estaba: no se escribe otra.
    let (n, escribir, orden) = nvram::decidir(&[(0, win), (5, o.clone())], &[5, 0], &g, &o);
    assert_eq!((n, escribir), (5, false));
    assert_eq!(orden, vec![5, 0]);
    // Basura.
    for i in 0..o.len() {
        let _ = nvram::leer_opcion(&o[..i]);
    }
}

#[test]
fn guid_texto_ida_y_vuelta() {
    let mut s = String::new();
    disco::guid_texto(&tipos::ESP, &mut s);
    assert_eq!(s, "C12A7328-F81F-11D2-BA4B-00A0C93EC93B");
    assert_eq!(disco::guid_de_texto(&s), Some(tipos::ESP));
    assert_eq!(disco::guid_de_texto("C12A7328-F81F-11D2-BA4B-00A0C93EC93"), None);
}

/// Un disco de verdad (solo lectura), si se da.
#[test]
fn disco_de_verdad() {
    let Ok(ruta) = std::env::var("DASHOS_DISCO") else { return };
    struct Fichero(std::fs::File, u64);
    impl Disco for Fichero {
        fn tam_sector(&self) -> usize {
            512
        }
        fn sectores(&self) -> u64 {
            self.1
        }
        fn leer(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
            use std::os::unix::fs::FileExt;
            self.0.read_exact_at(buf, lba * 512).map_err(|_| ())
        }
        fn escribir(&mut self, _: u64, _: &[u8]) -> Result<(), ()> {
            Err(())
        }
        fn vaciar(&mut self) -> Result<(), ()> {
            Ok(())
        }
    }
    let f = std::fs::File::open(&ruta).unwrap();
    let n = f.metadata().unwrap().len() / 512;
    let mut d = Fichero(f, n);
    let tabla = gpt::leer(&mut d).unwrap();
    let contenidos: Vec<_> = tabla.particiones.iter().map(|p| reconocer::reconocer(&mut d, p.desde, p.hasta)).collect();
    let v = DiscoVisto { nombre: "sata0".into(), modelo: "de verdad".into(), sectores: n, sector: 512, tabla, contenidos, de_arranque: false };
    println!("copias {:?}, avisos {:?}", v.tabla.copias, v.tabla.avisos);
    for (p, c) in v.tabla.particiones.iter().zip(&v.contenidos) {
        println!("{} [{}, {}) {:?}", plan::nombre_particion(p, Some(c)), p.desde, p.hasta, c);
    }
    for pedido in [Pedido::Hueco(None), Pedido::Encoger(3, 0), Pedido::Encoger(2, 0), Pedido::Encoger(1, 0)] {
        match plan::calcular(&v, pedido, &mut guids()) {
            Ok(p) => println!("{:?}:\n{}", pedido, plan::texto(&p, &v)),
            Err(e) => println!("{:?}: {}", pedido, e),
        }
    }
}

/// Un entorno de prueba: un "sistema" con la firma de DashFS, un cargador, y lo que se pide, apuntado.
struct Falso {
    sistema: Vec<u8>,
    cargador: Vec<u8>,
    nvram: Vec<(u32, u64, u64)>,
    registro: Vec<String>,
    azar: u64,
    encoger_falla: bool,
}

impl Falso {
    fn nuevo() -> Self {
        let mut sistema = vec![0x33u8; 3 << 20];
        sistema[..8].copy_from_slice(b"DashFS\0\0");
        Falso { sistema, cargador: vec![0xEF; 1_500_000], nvram: Vec::new(), registro: Vec::new(), azar: 7, encoger_falla: true }
    }
}

impl aplicar::Entorno for Falso {
    fn sistema(&mut self) -> &[u8] {
        &self.sistema
    }
    fn cargador(&mut self) -> &[u8] {
        &self.cargador
    }
    fn crecer(&mut self, d: &mut dyn Disco, desde: u64, _: u64) -> Result<(), String> {
        let mut s = vec![0u8; 512];
        d.leer(desde, &mut s).map_err(|_| String::from("no se lee"))?;
        if &s[..8] != b"DashFS\0\0" { Err("no está el sistema".into()) } else { Ok(()) }
    }
    fn encoger(&mut self, _: &mut dyn Disco, _: u64, _: u64, _: u64) -> Result<(), String> {
        if self.encoger_falla { Err("falta el dominio de x86".into()) } else { Ok(()) }
    }
    fn nvram(&mut self, n: u32, a: u64, b: u64, _: &[u8; 16]) -> Result<u16, String> {
        self.nvram.push((n, a, b));
        Ok(3)
    }
    fn apuntar(&mut self, l: &str) {
        self.registro.push(l.into());
    }
    fn azar(&mut self) -> u64 {
        self.azar = self.azar.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.azar
    }
}

/// Un "Windows": ESP con su cargador, MSR, C: (con bytes reconocibles) y recuperación al final, con hueco en medio.
fn windows(bytes: u64, fin_c: u64) -> Disperso {
    let fin = bytes / 512 - 2048;
    let rec = fin - 1024 * MIB / 2048 * 2048 / 2048 * 2048;
    let mut d = disco_con(bytes, &[(tipos::ESP, MIB, 101 * MIB), (tipos::MS_RESERVADA, 101 * MIB, 117 * MIB), (tipos::MS_DATOS, 117 * MIB, fin_c), (tipos::MS_RECUPERACION, rec, fin)]);
    fat::formatear_fat32(&mut d, MIB, 101 * MIB, "SYSTEM", 0x1234).unwrap();
    let mut f = fat::Fat::abrir(&mut d, MIB, 101 * MIB).unwrap();
    let efi = f.cajon(&mut d, 0, "EFI").unwrap();
    let ms = f.cajon(&mut d, efi, "Microsoft").unwrap();
    let boot = f.cajon(&mut d, ms, "Boot").unwrap();
    f.poner_fichero(&mut d, boot, "bootmgfw.efi", &vec![0x4D; 200_000]).unwrap();
    for s in [117 * MIB, 117 * MIB + 1, fin_c - 1, rec, fin - 1] {
        d.escribir(s, &[0xC0 + (s % 7) as u8; 512]).unwrap();
    }
    d
}

/// Lo de Windows (todas sus particiones y su cargador) sigue igual.
fn windows_intacto(d: &mut Disperso, antes: &Disperso, fin_c: u64) {
    let t = gpt::leer(d).unwrap();
    for q in gpt::leer(&mut Disperso { datos: antes.datos.clone(), ..Disperso::nuevo(antes.sectores * 512) }).unwrap().particiones {
        assert!(t.particiones.contains(&q), "falta {:?}", q);
        if q.tipo != tipos::ESP {
            for (s, v) in antes.datos.range(q.desde..q.hasta) {
                assert_eq!(d.datos.get(s), Some(v), "cambió el sector {} de {:?}", s, q.nombre);
            }
        }
    }
    let f = fat::Fat::abrir(d, MIB, 101 * MIB).unwrap();
    let efi = f.buscar(d, 0, "EFI").unwrap().unwrap();
    let ms = f.buscar(d, efi.cluster, "Microsoft").unwrap().unwrap();
    let boot = f.buscar(d, ms.cluster, "Boot").unwrap().unwrap();
    let b = f.buscar(d, boot.cluster, "bootmgfw.efi").unwrap().unwrap();
    assert_eq!(f.leer(d, &b).unwrap(), vec![0x4D; 200_000]);
    let _ = fin_c;
}

#[test]
fn aplicar_hueco_junto_a_windows() {
    let fin_c = 30 * 1024 * MIB;
    let base = windows(64 * GB, fin_c);
    let mut d = Disperso { datos: base.datos.clone(), ..Disperso::nuevo(64 * GB) };
    let v = aplicar::ver(&mut d, "sata0", "prueba", &[0; 16]).unwrap();
    let p = plan::calcular(&v, Pedido::Hueco(None), &mut guids()).unwrap();
    let mut e = Falso::nuevo();
    aplicar::aplicar(&mut d, &p, "sata0", "prueba", &[0; 16], &mut e).unwrap();
    windows_intacto(&mut d, &base, fin_c);
    let t = gpt::leer(&mut d).unwrap();
    assert_eq!(t.copias, Copias::Iguales);
    let ds = t.particiones.iter().find(|q| q.tipo == tipos::DASHOS).unwrap();
    assert_eq!((ds.desde, ds.hasta), (p.dashos.0, p.dashos.1));
    assert_eq!(e.nvram, vec![(1, MIB, 101 * MIB)]);
    let f = fat::Fat::abrir(&mut d, MIB, 101 * MIB).unwrap();
    let efi = f.buscar(&mut d, 0, "EFI").unwrap().unwrap();
    let ds = f.buscar(&mut d, efi.cluster, "DashOS").unwrap().unwrap();
    let c = f.buscar(&mut d, ds.cluster, "DASHOS.EFI").unwrap().unwrap();
    assert_eq!(f.leer(&mut d, &c).unwrap(), e.cargador);
    let tb = f.buscar(&mut d, ds.cluster, "TABLAS.BIN").unwrap().unwrap();
    assert!(f.leer(&mut d, &tb).unwrap().starts_with(b"DashOS-tablas 1\n"));
    // Repetir el mismo plan: el disco cambió (ya está), y no se hace.
    assert!(aplicar::aplicar(&mut d, &p, "sata0", "prueba", &[0; 16], &mut Falso::nuevo()).is_err());
    // Reinstalar sí.
    let v = aplicar::ver(&mut d, "sata0", "prueba", &[0; 16]).unwrap();
    let p2 = plan::calcular(&v, Pedido::Reinstalar, &mut guids()).unwrap();
    aplicar::aplicar(&mut d, &p2, "sata0", "prueba", &[0; 16], &mut Falso::nuevo()).unwrap();
    windows_intacto(&mut d, &base, fin_c);
}

/// El corte de luz en cada sector de la instalación: Windows sigue entero, y la instalación se puede repetir (en hueco
/// o reinstalando) y acaba bien.
#[test]
fn aplicar_cortado_en_cada_sector() {
    let fin_c = 30 * 1024 * MIB;
    let base = windows(64 * GB, fin_c);
    let mut d0 = Disperso { datos: base.datos.clone(), ..Disperso::nuevo(64 * GB) };
    let v = aplicar::ver(&mut d0, "sata0", "prueba", &[0; 16]).unwrap();
    let p = plan::calcular(&v, Pedido::Hueco(None), &mut guids()).unwrap();
    let mut e = Falso::nuevo();
    // Un sistema y cargador pequeños, para que el recorrido no sea eterno.
    e.sistema.truncate(64 * 1024);
    e.cargador.truncate(20_000);
    aplicar::aplicar(&mut d0, &p, "sata0", "prueba", &[0; 16], &mut e).unwrap();
    let total = d0.escrituras;
    assert!(total > 100);
    for corte in (0..total).step_by(1) {
        let mut d = Disperso { datos: base.datos.clone(), ..Disperso::nuevo(64 * GB) };
        d.corte = Some(corte);
        let mut e = Falso::nuevo();
        e.sistema.truncate(64 * 1024);
        e.cargador.truncate(20_000);
        assert!(aplicar::aplicar(&mut d, &p, "sata0", "prueba", &[0; 16], &mut e).is_err(), "corte {}", corte);
        d.corte = None;
        windows_intacto(&mut d, &base, fin_c);
        // Repetir: lo que toque según quedó.
        let v = aplicar::ver(&mut d, "sata0", "prueba", &[0; 16]).unwrap();
        let pedido = if v.tabla.particiones.iter().any(|q| q.tipo == tipos::DASHOS) && v.tabla.copias == Copias::Iguales { Pedido::Reinstalar } else { Pedido::Hueco(None) };
        let p2 = plan::calcular(&v, pedido, &mut guids()).unwrap_or_else(|m| panic!("corte {}: {} ({:?})", corte, m, v.tabla.copias));
        aplicar::aplicar(&mut d, &p2, "sata0", "prueba", &[0; 16], &mut e).unwrap_or_else(|m| panic!("corte {}: {}", corte, m));
        windows_intacto(&mut d, &base, fin_c);
        let t = gpt::leer(&mut d).unwrap();
        assert_eq!(t.copias, Copias::Iguales, "corte {}", corte);
        assert_eq!(t.particiones.iter().filter(|q| q.tipo == tipos::DASHOS).count(), 1, "corte {}", corte);
    }
}

#[test]
fn aplicar_disco_entero_y_encoger_sin_dominio() {
    let mut d = Disperso::nuevo(64 * GB);
    let v = aplicar::ver(&mut d, "nvme0", "vacío", &[0; 16]).unwrap();
    let p = plan::calcular(&v, Pedido::DiscoEntero, &mut guids()).unwrap();
    let mut e = Falso::nuevo();
    aplicar::aplicar(&mut d, &p, "nvme0", "vacío", &[0; 16], &mut e).unwrap();
    let t = gpt::leer(&mut d).unwrap();
    assert_eq!(t.copias, Copias::Iguales);
    assert_eq!(t.particiones.len(), 2);
    assert_eq!(e.nvram.len(), 1);
    // El pendrive del que arrancó: no.
    let g = t.particiones[0].guid;
    assert!(aplicar::aplicar(&mut d, &p, "nvme0", "vacío", &g, &mut Falso::nuevo()).is_err());
    // Encoger, sin el dominio: se para antes de tocar ninguna tabla.
    let base = windows(64 * GB, 63 * 1024 * MIB - 2048);
    let mut d = Disperso { datos: base.datos.clone(), ..Disperso::nuevo(64 * GB) };
    let mut v = aplicar::ver(&mut d, "sata0", "prueba", &[0; 16]).unwrap();
    // El reconocedor no ve NTFS en estos sectores falsos: se le dice que lo es, limpio y medio vacío.
    let k = v.tabla.particiones.iter().position(|q| q.tipo == tipos::MS_DATOS).unwrap();
    let ntfs = reconocer::Ntfs { cluster: 4096, clusters: 15_000_000, usados: Some(2_000_000), ultimo_usado: Some(15_000_000), sucio: Some(false), hibernado: Some(false), etiqueta: "Windows".into() };
    v.contenidos[k] = reconocer::Contenido::Ntfs(ntfs.clone());
    let p = plan::calcular(&v, Pedido::Encoger(3, 0), &mut guids()).unwrap();
    assert_eq!(p.modo, Modo::Encoger);
    let antes = d.escrituras;
    // `aplicar` relee el disco y no ve NTFS: el plan no casa, no se escribe nada.
    assert!(aplicar::aplicar(&mut d, &p, "sata0", "prueba", &[0; 16], &mut Falso::nuevo()).is_err());
    assert_eq!(d.escrituras, antes);
    // Las paradas de antes de encoger.
    for (cambio, dice) in [
        (reconocer::Ntfs { hibernado: Some(true), ..ntfs.clone() }, "hibernado"),
        (reconocer::Ntfs { sucio: Some(true), ..ntfs.clone() }, "comprobación"),
        (reconocer::Ntfs { usados: Some(14_000_000), ..ntfs.clone() }, "casi lleno"),
        (reconocer::Ntfs { hibernado: None, ..ntfs.clone() }, "hibernado"),
    ] {
        v.contenidos[k] = reconocer::Contenido::Ntfs(cambio);
        let m = plan::calcular(&v, Pedido::Encoger(3, 0), &mut guids()).unwrap_err();
        assert!(m.contains(dice), "{}", m);
    }
    v.contenidos[k] = reconocer::Contenido::BitLocker;
    assert!(plan::calcular(&v, Pedido::Encoger(3, 0), &mut guids()).unwrap_err().contains("BitLocker"));
}
