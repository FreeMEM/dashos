//! `Rename`: cambia de nombre o mueve dentro de un volumen. `Rename viejo [TO|AS] nuevo`. No sobrescribe.

use crate::Orden;

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::RENAME);
    let (de, hacia) = (args.texto("FROM").unwrap_or(""), args.texto("TO").unwrap_or(""));
    match o.s.renombrar(de, hacia) {
        Ok(()) => o.fin(shell::RC_BIEN),
        Err(e) => {
            o.error_de(e, if e == dashos::protocolo::archivos::YA_EXISTE { hacia } else { de });
            o.fin(shell::RC_FALLO)
        }
    }
}
