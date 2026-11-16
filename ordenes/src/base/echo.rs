//! `Echo`: escribe sus argumentos. `Echo texto... [NOLINE]`.

use crate::{di, Orden};

pub fn principal(o: &mut Orden) -> u64 {
    let a = o.repartir("/M,NOLINE/S");
    let texto = a.varios_en(0).unwrap_or(&[]).join(" ");
    di!(o, "{}{}", texto, if a.si("NOLINE") { "" } else { "\n" });
    o.fin(shell::RC_BIEN)
}
