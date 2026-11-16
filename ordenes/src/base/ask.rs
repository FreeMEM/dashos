//! `Ask`: hace una pregunta y lee la respuesta. `Ask pregunta`: con una "s" (o "y") el código es 5 (para `If WARN`);
//! con otra cosa, 0.

use crate::{di, Orden};

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir("PROMPT/A");
    di!(o, "{} ", args.texto("PROMPT").unwrap_or(""));
    let r = o.leer_linea().unwrap_or_default();
    let si = matches!(r.trim().to_lowercase().as_str(), "s" | "si" | "sí" | "y" | "yes");
    o.fin(if si { shell::RC_AVISO } else { shell::RC_BIEN })
}
