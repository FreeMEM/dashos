//! Casos hostiles: todas las funciones con registros al azar (y la memoria de la caja estropeada a ratos), rutas
//! hechas para salirse de las raíces y caminos muy hondos. Nada debe caerse, y el servicio de archivos solo debe ver
//! nombres limpios y asas que él dio (`Espia`).

mod comun;
use comun::*;

use caja_dos::estructuras::*;
use caja_dos::*;

struct Azar(u64);

impl Azar {
    fn sig(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn hasta(&mut self, n: u64) -> u64 {
        self.sig() % n
    }
    fn de<T: Copy>(&mut self, v: &[T]) -> T {
        v[self.hasta(v.len() as u64) as usize]
    }
}

const RUTAS: &[&str] = &[
    "",
    ":",
    "/",
    "//",
    "RAM:",
    "RAM:a",
    "RAM:a/b",
    "ram:A/B/c",
    "RAM:/",
    "RAM://x",
    "RAM:..",
    "RAM:../..",
    "RAM:a/../../..",
    "RAM:.",
    "SYS:S/Startup-Sequence",
    "SYS:C/Dir",
    "CD0:Leeme",
    "Work:x/y/z",
    "a:b:c",
    "NIL:",
    "*",
    "CONSOLE:",
    "PROGDIR:x",
    "ENV:var",
    "X:",
    "Y:z",
    "LIBS:a",
    "Nada:",
    "RAM:a\u{7f}b",
    "RAM:ñ",
    "RAM:a b c",
    "RAM:a//b//c",
    "RAM:a/",
    "RAM:\u{1}",
];

/// Un banco con algo dentro y unas asignaciones (con un bucle).
fn preparar() -> (Banco, Vec<u32>, Vec<u32>) {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.poner(b.ram, "a/b/c", b"datos");
    b.dos.fs.fs.poner(b.work, "x/y/z", b"zzz");
    let mut cadenas = Vec::new();
    for r in RUTAS {
        cadenas.push(b.cad(r));
    }
    let largo = "n".repeat(300);
    cadenas.push(b.cad(&largo));
    cadenas.push(b.cad(&format!("RAM:{}", "d/".repeat(150))));
    for (n, r) in [("X", "Y:"), ("Y", "X:"), ("LIBS", "SYS:Libs")] {
        let (n, r) = (b.cad(n), b.cad(r));
        b.llamar(lvo::ASSIGN_PATH, &[n, r]);
    }
    let mut huecos = Vec::new();
    for _ in 0..8 {
        huecos.push(b.hueco(512));
    }
    (b, cadenas, huecos)
}

#[test]
fn registros_al_azar() {
    // Más semillas con DOS_FUZZ=n (por ejemplo, en `cargo test --release`).
    let semillas: u64 = std::env::var("DOS_FUZZ").ok().and_then(|v| v.parse().ok()).unwrap_or(6);
    for semilla in 1..=semillas {
        let (mut b, cadenas, huecos) = preparar();
        let mut az = Azar(0x9E37_79B9_7F4A_7C15 ^ semilla);
        let mut bptrs: Vec<u32> = vec![0, 1, 0x3FFF_FFFF, 0x4000_0000, u32::MAX];
        let mut dirs: Vec<u32> = vec![
            0,
            4,
            0xFFF,
            0x1000,
            TAM_MEMORIA as u32 - 2,
            TAM_MEMORIA as u32 - 260,
            0xFFFE_FFF0,
            0xFFFF_0000,
            0xFFFF_FFFC,
            u32::MAX,
        ];
        dirs.extend(&huecos);
        let estropear = semilla % 2 == 0;
        let mut bien = 0u32;
        for i in 0..40_000u32 {
            let n = if az.hasta(50) == 0 { az.hasta(200) as u16 } else { 5 + az.hasta(162) as u16 };
            let mut r = Registros::default();
            for k in 0..8 {
                r.d[k] = match az.hasta(10) {
                    0 => az.sig() as u32,
                    1 => az.hasta(16) as u32,
                    2 => az.de(&[0u32, u32::MAX, 1004, 1005, 1006, (-2i32) as u32, 0x100, 0x400, 0xFFFF_FFFE]),
                    3 | 4 => az.de(&cadenas),
                    5 | 6 => az.de(&bptrs),
                    _ => az.de(&dirs),
                };
                r.a[k] = az.de(&dirs);
            }
            // Que las longitudes no pidan megas a cada vuelta.
            if matches!(n, lvo::READ | lvo::WRITE | lvo::FREAD | lvo::FWRITE) && az.hasta(4) != 0 {
                r.d[3] &= 0x3FF;
            }
            let acc = b.dos.llamar(&mut b.caja, n, &mut r);
            if r.d[0] != 0 && r.d[0] != u32::MAX {
                bien += 1;
            }
            assert!(matches!(acc, Accion::Seguir | Accion::Ajena | Accion::Dormir { .. }));
            // Lo que parece un BPTR o una dirección se guarda para usarlo después.
            let d0 = r.d[0];
            if d0 != 0 && d0 != u32::MAX && (d0 as usize) < TAM_MEMORIA / 4 && bptrs.len() < 64 {
                bptrs.push(d0);
            }
            if d0 >= 0x1000 && (d0 as usize) < TAM_MEMORIA && dirs.len() < 64 {
                dirs.push(d0);
            }
            if estropear && i % 97 == 0 {
                // Bytes al azar en la memoria de la caja (estructuras de dos incluidas), pero no en el proceso.
                for _ in 0..16 {
                    let d = 0x10000 + az.hasta(TAM_MEMORIA as u64 - 0x20000) as usize;
                    if d.abs_diff(b.proceso as usize) > 512 {
                        b.caja.mem.datos[d] = az.sig() as u8;
                    }
                }
            }
            if i % 5000 == 0 {
                b.dos.tomar_avisos();
            }
        }
        // Que no todo haya fallado: algo de lo que se probó llegó lejos.
        assert!(bien > 2_000, "semilla {semilla}: solo {bien} llamadas con D0 de éxito");
        if semillas <= 6 {
            eprintln!(
                "semilla {semilla}: {bien} con éxito, {} cerrojos y {} ficheros abiertos",
                b.dos.cerrojos_abiertos(),
                b.dos.ficheros_abiertos()
            );
        }
        b.sin_faltas();
        // Al acabar la tarea no queda nada suyo.
        let t = b.proceso;
        b.dos.acabar_tarea(&mut b.caja, t);
        assert_eq!(b.dos.ficheros_abiertos(), 0);
        b.sin_faltas();
    }
}

#[test]
fn rutas_para_salirse() {
    let (mut b, _, _) = preparar();
    // Solo las rutas de RAM: o relativas (el cajón actual es RAM:a): las de otros volúmenes son legítimas.
    let mut cadenas = Vec::new();
    for r in RUTAS.iter().filter(|r| !["SYS:", "Work:", "CD0:", "LIBS:"].iter().any(|v| r.starts_with(v))) {
        cadenas.push(b.cad(r));
    }
    cadenas.push(b.cad(&"n".repeat(300)));
    let antes: Vec<bool> = ["S", "C", "Libs", "x", "Leeme"]
        .iter()
        .map(|r| b.dos.fs.fs.existe(b.sys, r) || b.dos.fs.fs.existe(b.work, r))
        .collect();
    let cd = b.lock("RAM:a", SHARED_LOCK);
    b.llamar(lvo::CURRENT_DIR, &[cd]);
    for &c in &cadenas {
        for &m in &[MODE_OLDFILE, MODE_NEWFILE, MODE_READWRITE] {
            let f = b.llamar(lvo::OPEN, &[c, m as u32]);
            if f != 0 {
                b.close(f);
            }
        }
        let l = b.llamar(lvo::LOCK, &[c, SHARED_LOCK as u32]);
        if l != 0 {
            let p = b.llamar(lvo::PARENT_DIR, &[l]);
            b.unlock(p);
            b.unlock(l);
        }
        let l = b.llamar(lvo::CREATE_DIR, &[c]);
        b.unlock(l);
        for &c2 in &cadenas[..6] {
            b.llamar(lvo::RENAME, &[c, c2]);
        }
        b.llamar(lvo::SET_COMMENT, &[c, c]);
        b.llamar(lvo::SET_PROTECTION, &[c, 0]);
        b.llamar(lvo::DELETE_FILE, &[c]);
    }
    b.llamar(lvo::CURRENT_DIR, &[0]);
    b.unlock(cd);
    b.sin_faltas();
    // Lo de los otros volúmenes sigue ahí (nada de RAM: ha llegado a ellos).
    let despues: Vec<bool> = ["S", "C", "Libs", "x", "Leeme"]
        .iter()
        .map(|r| b.dos.fs.fs.existe(b.sys, r) || b.dos.fs.fs.existe(b.work, r))
        .collect();
    assert_eq!(antes, despues);
    assert!(b.dos.fs.fs.existe(b.sys, "S/Startup-Sequence"));
    assert!(b.dos.fs.fs.existe(b.cd, "Leeme"));
}

#[test]
fn caminos_hondos_en_trozos() {
    let mut b = Banco::nuevo();
    // 40 cajones de 20 caracteres: 840 bytes de camino, más de lo que cabe en un mensaje (224).
    let mut ruta = String::from("Work:");
    for i in 0..40 {
        ruta.push_str(&format!("cajon-numero-{i:07}"));
        let n = b.cad(&ruta);
        let l = b.llamar(lvo::CREATE_DIR, &[n]);
        assert_ne!(l, 0, "{i}: {}", b.io_err());
        b.unlock(l);
        ruta.push('/');
    }
    ruta.push_str("fichero");
    let f = b.open(&ruta, MODE_NEWFILE);
    assert_ne!(f, 0);
    b.write(f, b"hondo");
    b.close(f);
    let l = b.lock(&ruta, SHARED_LOCK);
    assert_ne!(l, 0);
    let buf = b.hueco(1024);
    assert_eq!(b.llamar(lvo::NAME_FROM_LOCK, &[l, buf, 1024]), DOSTRUE);
    assert_eq!(b.cadena_en(buf), ruta);
    // Con un búfer corto, ERROR_LINE_TOO_LONG.
    assert_eq!(b.llamar(lvo::NAME_FROM_LOCK, &[l, buf, 100]), 0);
    assert_eq!(b.io_err(), caja_dos::errores::ERROR_LINE_TOO_LONG);
    // ParentDir hasta arriba.
    let mut x = b.llamar(lvo::DUP_LOCK, &[l]);
    let mut vueltas = 0;
    while x != 0 {
        let p = b.llamar(lvo::PARENT_DIR, &[x]);
        b.unlock(x);
        x = p;
        vueltas += 1;
    }
    assert_eq!(vueltas, 42);
    b.unlock(l);
    // Y de abajo arriba con "/".
    let n = b.cad(&ruta);
    let f = b.llamar(lvo::OPEN, &[n, MODE_OLDFILE as u32]);
    assert_eq!(b.read(f, 10).1, b"hondo");
    b.close(f);
    let subir = format!("{}{}", ruta.trim_end_matches("fichero"), "/".repeat(40));
    let l = b.lock(&subir, SHARED_LOCK);
    assert_eq!(b.name_from_lock(l), "Work:");
    b.unlock(l);
    b.limpio(0, 1);
}

#[test]
fn sin_asas_en_el_servicio() {
    let mut b = Banco::nuevo();
    b.dos.fs.fs.max_asas = 4;
    let mut ls = Vec::new();
    for _ in 0..4 {
        let l = b.lock("SYS:S", SHARED_LOCK);
        assert_ne!(l, 0);
        ls.push(l);
    }
    assert_eq!(b.lock("SYS:S", SHARED_LOCK), 0);
    assert_eq!(b.io_err(), caja_dos::errores::ERROR_NO_FREE_STORE);
    assert_eq!(b.open("RAM:x", MODE_NEWFILE), 0);
    let (r, _) = b.examine(ls[0]);
    assert_eq!(r, 0);
    for l in ls {
        b.unlock(l);
    }
    b.limpio(0, 1);
}
