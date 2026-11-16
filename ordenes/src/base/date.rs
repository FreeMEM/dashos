//! `Date`: la fecha y la hora (UTC: aún no hay zona horaria). No la cambia: es de la máquina, y cambiarla es una acción
//! de administrador (S6b).

use crate::{di, Orden};

pub fn principal(o: &mut Orden) -> u64 {
    let _ = o.repartir(shell::ordenes::DATE);
    let ns = dashos::hora_ns();
    if ns == 0 {
        o.error(format_args!("el sistema no sabe la hora"));
        return o.fin(shell::RC_AVISO);
    }
    di!(o, "{}\n", shell::fecha::texto(ns / 1_000_000_000));
    o.fin(shell::RC_BIEN)
}
