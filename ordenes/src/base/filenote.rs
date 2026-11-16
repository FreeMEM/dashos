//! `Filenote`: pone (o quita) el comentario de un fichero. `Filenote fichero [comentario]`.

use crate::Orden;

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir("FILE/A,COMMENT");
    let fichero = args.texto("FILE").unwrap_or("");
    let comentario = args.texto("COMMENT").unwrap_or("");
    if comentario.len() > 224 {
        o.error(format_args!("el comentario no cabe (224 bytes como mucho)"));
        return o.fin(shell::RC_FALLO);
    }
    match o.s.comentar(fichero, comentario) {
        Ok(()) => o.fin(shell::RC_BIEN),
        Err(e) => {
            o.error_de(e, fichero);
            o.fin(shell::RC_FALLO)
        }
    }
}
