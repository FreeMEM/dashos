//! La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md): lo que la guardia de ficheros (S28, `caja_dos::guardia`) no deja
//! cambiar (iconos, ejecutables, guiones de arranque que ya estaban) se pregunta al usuario por la terminal
//! (`/dev/tty`), como en DashOS por `sesiones` y `guardian`. Sin terminal, se niega. Con `CAJA_AMIGA_PROTEGER=no`, no
//! hay guardia que pregunte: se deja todo (para guiones y pruebas, que no tienen a quién preguntar).

use alloc::format;

use caja_dos::guardia::{Clase, Preguntador};

use crate::sys::{self, Camino};

/// Lo que dice `cuenta` cuando no se ha preguntado nada (como en DashOS).
pub const NADA: u32 = 0xff;

pub fn reiniciar_cuenta() {}

pub fn cuenta() -> (u32, u32) {
    (0, NADA)
}

pub fn apuntar(_que: &str, _motivo: &str) {}

/// Si la guardia tiene que preguntar (y si no, dejarlo todo): no con `CAJA_AMIGA_PROTEGER=no`.
pub fn proteger() -> bool {
    sys::entorno(b"CAJA_AMIGA_PROTEGER\0") != Some(b"no")
}

/// Pregunta por la terminal.
pub struct PorTerminal;

impl Preguntador for PorTerminal {
    fn preguntar(&mut self, programa: &str, clase: Clase, nombre: &str) -> bool {
        let Some(tty) = Camino::de("/dev/tty") else { return false };
        let Ok(fd) = sys::openat(sys::AT_FDCWD, &tty, sys::O_RDWR, 0) else { return false };
        let p = format!(
            "caja-amiga: {} quiere cambiar o borrar \"{}\" ({}), que ya estaba. ¿Lo dejas? [s/N] ",
            programa,
            nombre,
            clase.nombre()
        );
        let _ = sys::escribir_fd(fd, p.as_bytes());
        let mut b = [0u8; 16];
        let n = sys::leer_fd(fd, &mut b).unwrap_or(0);
        sys::close(fd);
        n > 0 && matches!(b[0], b's' | b'S' | b'y' | b'Y')
    }
}

/// Deja todo (`CAJA_AMIGA_PROTEGER=no`).
pub struct Nadie;

impl Preguntador for Nadie {
    fn preguntar(&mut self, _programa: &str, _clase: Clase, _nombre: &str) -> bool {
        true
    }
}
