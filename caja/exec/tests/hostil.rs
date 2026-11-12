//! A lo bruto: todas las funciones con registros al azar, con la memoria del sistema estropeada a ratos. Nada
//! puede entrar en pánico (las pruebas van con comprobación de desbordes, como el modo de depuración).

mod comun;
use caja_exec::estructuras::*;
use caja_exec::{Accion, Memoria, Registros, lvo};
use comun::*;

struct Azar(u64);

impl Azar {
    fn n(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as u32
    }
    fn hasta(&mut self, n: u32) -> u32 {
        self.n() % n
    }
}

/// Un valor de registro "con mala idea": cero, la página 0, los bordes de la memoria, lo más alto, un puntero a
/// algo de verdad o cualquier cosa.
fn valor(a: &mut Azar, utiles: &[u32]) -> u32 {
    match a.hasta(10) {
        0 => 0,
        1 => a.hasta(0x1000),
        2 => TAMANO - a.hasta(64),
        3 => u32::MAX - a.hasta(64),
        4 => 0xFFFF_0000 - a.hasta(64),
        5 | 6 if !utiles.is_empty() => utiles[a.hasta(utiles.len() as u32) as usize].wrapping_add(a.hasta(3) * 2),
        7 => a.hasta(64),
        8 => (a.n() as i8) as i32 as u32,
        _ => a.n(),
    }
}

fn ronda(semilla: u64, llamadas: u32, estropear: bool) {
    let (mut e, t) = caja_con_tarea();
    let mut a = Azar(semilla);
    let b = e.base();
    let mut utiles = vec![b, t, b + execbase::LIBLIST, b + execbase::PORTLIST, b + execbase::TASKREADY];
    utiles.extend(e.nodos(b + execbase::MEMLIST).unwrap());
    for _ in 0..4 {
        let p = e.create_msg_port().unwrap();
        utiles.push(p);
        utiles.push(e.alloc_mem(64, memf::CLEAR).unwrap());
        utiles.push(cadena(&mut e, "exec.library"));
    }
    for i in 0..llamadas {
        if e.tarea_actual() == 0 || e.tareas().is_empty() {
            if e.tareas().is_empty() {
                let nt = e.crear_tarea("otra", 0, 1024, 0x1000);
                if let Ok(nt) = nt {
                    utiles.push(nt);
                }
            }
            if e.elegir_siguiente().is_none() {
                // Todas esperan: se despierta a una cualquiera.
                if let Some(&x) = e.tareas().first() {
                    let _ = e.signal(x, u32::MAX);
                }
                let _ = e.elegir_siguiente();
            }
        }
        if estropear && i % 50 == 0 {
            // Un programa escribe donde no debe: en ExecBase, sus listas o una región.
            let d = valor(&mut a, &utiles);
            let v = a.n();
            let _ = e.mem.escribir32(d & !1, v);
        }
        let n = match a.hasta(8) {
            0 => lvo::FIN_DE_TAREA,
            1 => a.hasta(300) as u16,
            _ => 1 + a.hasta(lvo::ENTRADAS as u32) as u16,
        };
        let mut r = Registros::default();
        for k in 0..8 {
            r.d[k] = valor(&mut a, &utiles);
            r.a[k] = valor(&mut a, &utiles);
        }
        let mut acc = e.llamar(n, &mut r);
        let mut vueltas = 0;
        while let Accion::Llamar { regs, tras, .. } = acc {
            let mut hecho = regs;
            hecho.d[0] = valor(&mut a, &utiles);
            acc = e.continuar(tras, &hecho, &mut r);
            vueltas += 1;
            assert!(vueltas < 4);
        }
        match acc {
            Accion::Ceder | Accion::Bloquear | Accion::Terminar => {
                let _ = e.elegir_siguiente();
            }
            _ => {}
        }
        if a.hasta(20) == 0 && e.fin_de_turno() {
            let _ = e.elegir_siguiente();
        }
        if a.hasta(500) == 0
            && let Some(&x) = e.tareas().first()
        {
            let _ = e.matar_tarea(x);
            let _ = e.elegir_siguiente();
        }
        e.tomar_avisos();
        e.tomar_quitadas();
        e.tomar_codigo_cambiado();
    }
}

#[test]
fn registros_al_azar() {
    for semilla in 1..=8 {
        ronda(0x9E37_79B9_7F4A_7C15 ^ semilla, 20_000, false);
    }
}

#[test]
fn registros_al_azar_con_la_memoria_estropeada() {
    for semilla in 1..=8 {
        ronda(0xD1B5_4A32_D192_ED03 ^ semilla, 20_000, true);
    }
}

#[test]
fn memoria_minima() {
    // La caja más pequeña con la que arranca, y luego todo al azar.
    let tam = 0x8000;
    let mut e =
        caja_exec::Exec::arrancar(caja_exec::MemoriaVec::nueva(tam), caja_exec::Config::para(tam as u32)).unwrap();
    let mut a = Azar(7);
    for _ in 0..20_000 {
        let mut r = Registros::default();
        for k in 0..8 {
            r.d[k] = valor(&mut a, &[]);
            r.a[k] = valor(&mut a, &[]);
        }
        let n = 1 + a.hasta(lvo::ENTRADAS as u32) as u16;
        let _ = e.llamar(n, &mut r);
        let _ = e.elegir_siguiente();
    }
}
