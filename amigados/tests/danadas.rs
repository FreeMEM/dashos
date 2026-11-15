//! R13 (regla 4 de `docs/diseño/S6b-usuarios-y-permisos.md`): un disco ajeno hecho a mala idea no puede tumbar a quien
//! lo lee ni dejarlo colgado. Se toma el disquete de las pruebas (`kernel/pruebas/disquete.adf.gz`), se le cambian
//! bytes al azar (sobre todo en el bloque raíz, el de arranque y los de cabecera) y se recorre entero: abrir, listar
//! cada cajón, leer cada fichero y contar los bloques libres. Tiene que acabar sin pánico, en poco tiempo. Como haría
//! quien lo prepara a mala idea, las sumas de control de los bloques dañados se rehacen: si no, el lector los rechaza
//! a la primera y no se prueba nada más.

use amigados::{EnMemoria, Tipo, Volumen};
use std::time::{Duration, Instant};

fn disquete() -> Vec<u8> {
    let gz = concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/pruebas/disquete.adf.gz");
    let salida = std::process::Command::new("gzip").args(["-dc", gz]).output().expect("sin gzip");
    assert!(salida.status.success() && salida.stdout.len() == 901120, "el disquete de las pruebas");
    salida.stdout
}

/// Un generador de números al azar pequeño y fijo: la misma semilla, los mismos daños.
struct Azar(u64);
impl Azar {
    fn sig(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Todo lo que haría un lector: cuántas entradas y bytes vio (para que no se optimice nada).
fn recorrer(img: &[u8]) -> (usize, u64) {
    let Ok(mut v) = Volumen::abrir(EnMemoria(img)) else { return (0, 0) };
    let _ = v.libres();
    let Ok(raiz) = v.raiz() else { return (0, 0) };
    let (mut entradas, mut bytes) = (0usize, 0u64);
    let mut pendientes = vec![(raiz, 0)];
    let mut buf = vec![0u8; 16 * 1024];
    while let Some((cajon, hondo)) = pendientes.pop() {
        // Un disco puede decir que un cajón está dentro de sí mismo: el lector tiene que acabar, y quien recorre pone
        // su propio tope (como hará el servicio).
        if hondo > 16 || entradas > 10_000 {
            break;
        }
        let Ok(lista) = v.listar(&cajon) else { continue };
        for e in lista {
            entradas += 1;
            match e.tipo {
                Tipo::Cajon => pendientes.push((e, hondo + 1)),
                Tipo::Fichero => {
                    let mut desde = 0u64;
                    while let Ok(n) = v.leer_de(&e, desde, &mut buf) {
                        if n == 0 || desde > 4 << 20 {
                            break;
                        }
                        bytes += n as u64;
                        desde += n as u64;
                    }
                }
                Tipo::Enlace => {
                    let _ = v.camino(&cajon, &e.nombre);
                }
            }
        }
    }
    (entradas, bytes)
}

#[test]
fn disquetes_danados_no_tumban_al_lector() {
    let bueno = disquete();
    let (n, _) = recorrer(&bueno);
    assert!(n > 0, "el disquete bueno tiene que listarse");
    let mut azar = Azar(0x9E37_79B9_7F4A_7C15);
    let empiezo = Instant::now();
    let mut peor = Duration::ZERO;
    let mut abiertos = 0;
    for _ in 0..20_000 {
        let mut img = bueno.clone();
        for _ in 0..1 + azar.sig() % 24 {
            // La mitad de los daños, en los bloques que más importan: arranque (0, 1), raíz (880) y los de alrededor.
            let bloque = match azar.sig() % 4 {
                0 => azar.sig() % 2,
                1 => 880,
                2 => 870 + azar.sig() % 40,
                _ => azar.sig() % 1760,
            } as usize;
            let i = bloque * 512 + (azar.sig() % 512) as usize;
            img[i] = match azar.sig() % 3 {
                0 => 0,
                1 => 0xFF,
                _ => azar.sig() as u8,
            };
        }
        // Rehacer la suma (el entero de la posición 20: que todos los del bloque sumen 0) de cada bloque tocado.
        for b in 2..1760 {
            if img[b * 512..b * 512 + 512] != bueno[b * 512..b * 512 + 512] && azar.sig() % 8 != 0 {
                let bl = &mut img[b * 512..b * 512 + 512];
                bl[20..24].fill(0);
                let suma = bl.chunks_exact(4).fold(0u32, |s, w| s.wrapping_add(u32::from_be_bytes(w.try_into().unwrap())));
                bl[20..24].copy_from_slice(&0u32.wrapping_sub(suma).to_be_bytes());
            }
        }
        let t = Instant::now();
        if recorrer(&img).0 > 0 {
            abiertos += 1;
        }
        peor = peor.max(t.elapsed());
    }
    eprintln!("20.000 disquetes dañados en {:?} ({} se listan); el más lento, {:?}", empiezo.elapsed(), abiertos, peor);
    assert!(abiertos > 3000, "casi ninguno se abre: la prueba no llega dentro");
    assert!(peor < Duration::from_secs(2), "un disquete dañado deja al lector casi colgado: {:?}", peor);
}
