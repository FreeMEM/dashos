//! `MakeDir`: crea cajones. `MakeDir nombre...`.

use dashos::protocolo::archivos as a;
use crate::Orden;

pub fn principal(o: &mut Orden) -> u64 {
    let args = o.repartir(shell::ordenes::MAKEDIR);
    let mut rc = shell::RC_BIEN;
    for n in args.varios("NAME") {
        match o.s.crear(n, a::CAJON) {
            Ok(asa) => {
                o.s.cerrar(asa);
            }
            Err(e) => {
                o.error_de(e, n);
                rc = shell::RC_FALLO;
            }
        }
    }
    o.fin(rc)
}
