//! `Date`: la fecha y la hora, en la zona horaria del sistema (la de la barra), y detrás la zona y cómo va respecto a
//! UTC: `2026-11-16 09:41:30 (Europe/Madrid, UTC+1)`; sin zona, `(UTC)`. No la cambia: es de la máquina, y cambiarla es
//! una acción de administrador (S6b).

use crate::{di, Orden};

pub fn principal(o: &mut Orden) -> u64 {
    let _ = o.repartir(shell::ordenes::DATE);
    let ns = dashos::hora_ns();
    if ns == 0 {
        o.error(format_args!("el sistema no sabe la hora"));
        return o.fin(shell::RC_AVISO);
    }
    let s = ns / 1_000_000_000;
    let local = shell::fecha::texto(o.s.local_s(s));
    match o.s.zona() {
        Some(z) => {
            let mut e = [0u8; 10];
            let n = dashos::zona::etiqueta(z, s, &mut e);
            let etiqueta = core::str::from_utf8(&e[..n]).unwrap_or("?");
            di!(o, "{} ({}, {})\n", local, z.nombre, etiqueta);
        }
        None => di!(o, "{} (UTC)\n", local),
    }
    o.fin(shell::RC_BIEN)
}
