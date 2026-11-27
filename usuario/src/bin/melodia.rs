//! Un reproductor de prueba (encargo `sonido-pro`): toca, sintetizadas aquí, dos piezas cortas que se repiten (bajo,
//! arpegio y melodía, compuestas para DashOS; MPL-2.0 como el resto), dice al servicio lo que suena (`MEDIOS`) y
//! obedece al mezclador: reproducir o pausar, siguiente y anterior. Sirve para ver el mezclador, el visualizador y
//! los controles de medios con algo que suena de verdad, y para medir la CPU del grafo con música.
//!
//! Llave: "sonido" (por su nombre).

#![no_std]
#![no_main]

extern crate alloc;

#[path = "../sonido_cliente.rs"]
mod sonido_cliente;

use alloc::vec::Vec;
use dashos::println;
use dashos::protocolo::sonido as p;
use sonido_cliente::Llega;

dashos::programa!(principal);
dashos::monton!();

const FS: u32 = 48_000;
const VA_ANILLO: usize = 0x41_0000_0000;
const TAM: usize = 32 * 1024;
const TROZO: usize = 1024;

/// Una pieza: tempo, acordes (raíz MIDI y si es menor) de cada compás, y la melodía en semicorcheas (nota MIDI, 0 =
/// silencio, 1 = sigue la anterior).
struct Pieza {
    titulo: &'static str,
    tempo: u32,
    acordes: &'static [(u8, bool)],
    melodia: &'static [u8],
}

const PIEZAS: [Pieza; 2] = [
    Pieza {
        titulo: "Amanecer en Kickstart",
        tempo: 112,
        acordes: &[(57, true), (53, false), (60, false), (55, false)],
        melodia: &[
            76, 1, 1, 1, 74, 1, 72, 1, 74, 1, 1, 1, 76, 1, 79, 1, //
            77, 1, 1, 1, 76, 1, 74, 1, 72, 1, 1, 1, 0, 0, 72, 74, //
            76, 1, 1, 1, 79, 1, 1, 1, 84, 1, 83, 1, 79, 1, 76, 1, //
            74, 1, 1, 1, 1, 1, 71, 1, 74, 1, 1, 1, 0, 0, 0, 0,
        ],
    },
    Pieza {
        titulo: "Copper y lluvia",
        tempo: 92,
        acordes: &[(50, true), (58, false), (53, false), (60, false)],
        melodia: &[
            69, 1, 1, 1, 1, 1, 70, 1, 72, 1, 1, 1, 70, 1, 69, 1, //
            65, 1, 1, 1, 1, 1, 1, 1, 0, 0, 62, 1, 65, 1, 70, 1, //
            69, 1, 1, 1, 1, 1, 65, 1, 67, 1, 1, 1, 69, 1, 72, 1, //
            72, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0,
        ],
    },
];

/// Una voz: fase, paso (de la nota) y envolvente.
#[derive(Clone, Copy, Default)]
struct Voz {
    fase: u32,
    paso: u32,
    env: i32,
    caida: i32,
}

impl Voz {
    fn tocar(&mut self, paso: u32, caida: i32) {
        self.paso = paso;
        self.env = 32_767;
        self.caida = caida;
    }
    fn avanzar(&mut self) {
        self.fase = self.fase.wrapping_add(self.paso);
        self.env = (self.env as i64 * self.caida as i64 >> 15) as i32;
    }
}

struct Sinte {
    seno: Vec<i32>,
    pasos: Vec<u32>,
    bajo: Voz,
    arpegio: Voz,
    melodia: Voz,
    eco: Vec<[i32; 2]>,
    pos_eco: usize,
    vibrato: u32,
}

impl Sinte {
    fn nuevo() -> Self {
        let seno = (0..1024).map(|i| (sonido::mates::seno(2.0 * core::f64::consts::PI * i as f64 / 1024.0) * 32767.0) as i32).collect();
        // Los pasos de cada nota MIDI: 440 · 2^((n-69)/12) Hz, en fase de 32 bits.
        let pasos = (0..128)
            .map(|n| {
                let hz = 440.0 * sonido::mates::exp((n as f64 - 69.0) / 12.0 * core::f64::consts::LN_2);
                (hz * 4_294_967_296.0 / FS as f64) as u32
            })
            .collect();
        // Un eco de 3/8 de negra a ~112 bpm (~200 ms).
        Sinte { seno, pasos, bajo: Voz::default(), arpegio: Voz::default(), melodia: Voz::default(), eco: alloc::vec![[0; 2]; 9600], pos_eco: 0, vibrato: 0 }
    }

    fn s(&self, fase: u32) -> i32 {
        self.seno[(fase >> 22) as usize]
    }

    /// Una muestra estéreo.
    fn muestra(&mut self) -> [i32; 2] {
        // Bajo: seno con su octava (más cuerpo), al centro.
        let b = (self.s(self.bajo.fase) + self.s(self.bajo.fase.wrapping_mul(2)) / 3) as i64 * self.bajo.env as i64 >> 15;
        // Arpegio: pulso suave (seno saturado), algo a la izquierda, con eco que rebota.
        let a0 = self.s(self.arpegio.fase);
        let a = (a0.clamp(-20_000, 20_000) * 3 / 2) as i64 * self.arpegio.env as i64 >> 15;
        // Melodía: seno con un vibrato lento, algo a la derecha.
        self.vibrato = self.vibrato.wrapping_add(5 * 89_478);
        let vib = self.s(self.vibrato) as i64 * (self.melodia.paso as i64 / 600) >> 15;
        let m = self.s(self.melodia.fase) as i64 * self.melodia.env as i64 >> 15;
        self.melodia.fase = self.melodia.fase.wrapping_add(vib as i32 as u32);
        self.bajo.avanzar();
        self.arpegio.avanzar();
        self.melodia.avanzar();
        let e = self.eco[self.pos_eco];
        let l = b * 30 / 100 + a * 22 / 100 + m * 18 / 100 + e[1] as i64 * 40 / 100;
        let r = b * 30 / 100 + a * 12 / 100 + m * 26 / 100 + e[0] as i64 * 40 / 100;
        self.eco[self.pos_eco] = [(a * 22 / 100 + e[1] as i64 * 35 / 100) as i32, (a * 12 / 100 + e[0] as i64 * 35 / 100) as i32];
        self.pos_eco = (self.pos_eco + 1) % self.eco.len();
        [l.clamp(-32_767, 32_767) as i32, r.clamp(-32_767, 32_767) as i32]
    }
}

fn principal() -> u64 {
    dashos::monton::con_nombre(&"melodia");
    let Some(canal) = dashos::llaves::buscar("sonido") else {
        println!("  [melodia] sin la llave del sonido");
        return 1;
    };
    let Some(mut f) = sonido_cliente::abrir(canal, sonido_cliente::I16, 2, FS, VA_ANILLO, TAM) else {
        println!("  [melodia] no se puede abrir el flujo");
        return 1;
    };
    let mut sinte = Sinte::nuevo();
    let mut pieza = 0usize;
    let mut sonando = true;
    let mut semicorchea = 0usize;
    let mut dentro = 0u32;
    let mut tocadas: u64 = 0;
    let caps = p::PUEDE_PAUSAR | p::PUEDE_SIGUIENTE | p::PUEDE_ANTERIOR;
    let duracion = |k: usize| PIEZAS[k].melodia.len() as u32 * 60_000 / PIEZAS[k].tempo / 4;
    f.anunciar(p::MED_SUENA, caps, 0, duracion(pieza), PIEZAS[pieza].titulo, "DashOS");
    println!("  [melodia] suena «{}»", PIEZAS[pieza].titulo);
    let mut trozo = alloc::vec![0u8; TROZO * 4];
    let mut ultimo_anuncio = 0u64;
    loop {
        // Lo que quepa, a trozos.
        while f.libre() >= trozo.len() {
            let pz = &PIEZAS[pieza];
            let largo = FS * 60 / pz.tempo / 4;
            for c in trozo.chunks_exact_mut(4) {
                if dentro == 0 && sonando {
                    let k = semicorchea % pz.melodia.len();
                    let compas = (k / 16) % pz.acordes.len();
                    let (raiz, menor) = pz.acordes[compas];
                    // Bajo en corcheas (raíz y quinta), arpegio en semicorcheas por las notas del acorde.
                    if k % 2 == 0 {
                        let nota = raiz as usize - 12 + if k % 8 == 4 { 7 } else { 0 };
                        sinte.bajo.tocar(sinte.pasos[nota], 32_740);
                    }
                    let tercera = if menor { 3 } else { 4 };
                    let arp = [0, tercera, 7, 12, 7, tercera][k % 6];
                    sinte.arpegio.tocar(sinte.pasos[raiz as usize + 12 + arp], 32_720);
                    match pz.melodia[k] {
                        0 => sinte.melodia.env = sinte.melodia.env / 2,
                        1 => {}
                        n => sinte.melodia.tocar(sinte.pasos[n as usize], 32_758),
                    }
                    semicorchea += 1;
                }
                dentro = (dentro + 1) % largo;
                let [l, r] = if sonando { sinte.muestra() } else { [0, 0] };
                c[..2].copy_from_slice(&(l as i16).to_le_bytes());
                c[2..].copy_from_slice(&(r as i16).to_le_bytes());
            }
            f.escribir(&trozo);
            tocadas += TROZO as u64;
        }
        // Cada segundo, la posición.
        if tocadas >= ultimo_anuncio + FS as u64 {
            ultimo_anuncio = tocadas;
            let pz = &PIEZAS[pieza];
            let pos = ((semicorchea % pz.melodia.len()) as u32) * 60_000 / pz.tempo / 4;
            f.anunciar(if sonando { p::MED_SUENA } else { p::MED_PAUSA }, caps, pos, duracion(pieza), pz.titulo, "DashOS");
        }
        match f.esperar_algo() {
            Llega::Muestras => {}
            Llega::Orden(o) => {
                match o {
                    p::ORDEN_REPRODUCIR_PAUSAR => sonando = !sonando,
                    p::ORDEN_SIGUIENTE | p::ORDEN_ANTERIOR => {
                        pieza = (pieza + 1) % PIEZAS.len();
                        semicorchea = 0;
                        dentro = 0;
                        sonando = true;
                    }
                    _ => {}
                }
                println!("  [melodia] orden {}: {} «{}»", o, if sonando { "suena" } else { "en pausa" }, PIEZAS[pieza].titulo);
                f.anunciar(if sonando { p::MED_SUENA } else { p::MED_PAUSA }, caps, 0, duracion(pieza), PIEZAS[pieza].titulo, "DashOS");
            }
        }
    }
}
