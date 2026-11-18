//! `listar_cajon` y `nombres_de_dispositivos` (rama amiga-apps: el pedidor de ficheros de asl.library nativa): lo de un
//! cajón, con las asignaciones múltiples juntas y sin repetir nombres; y lo hostil.

mod comun;

use comun::*;

#[test]
fn listar_un_cajon_y_una_multiasignacion() {
    let mut b = Banco::nuevo();
    let (work, ram) = (b.work, b.ram);
    b.fs().poner(work, "Musica/uno.mod", b"1234");
    b.fs().poner_cajon(work, "Musica/Viejos");
    b.fs().poner(ram, "Musica/dos.mod", b"12");
    b.fs().poner(ram, "Musica/uno.mod", b"repetido");
    let mut v = b.dos.listar_cajon(&mut b.caja, b"Work:Musica", 100).unwrap();
    v.sort();
    assert_eq!(v, vec![(b"Viejos".to_vec(), true, v[0].2), (b"uno.mod".to_vec(), false, 4)]);
    // Una multiasignación: lo de los dos sitios, sin repetir "uno.mod" (gana el primero).
    let mut cd = b.dos.listar_cajon(&mut b.caja, b"SYS:", 100).unwrap();
    cd.sort();
    assert!(cd.iter().any(|(n, c, _)| n == b"Libs" && *c));
    b.dos.asignar(&mut b.caja, "MOD", "Work:Musica").unwrap();
    b.dos.asignar_mas(&mut b.caja, "MOD", "RAM:Musica").unwrap();
    let mut m = b.dos.listar_cajon(&mut b.caja, b"MOD:", 100).unwrap();
    m.sort();
    let nombres: Vec<&[u8]> = m.iter().map(|(n, _, _)| n.as_slice()).collect();
    assert_eq!(nombres, [&b"Viejos"[..], b"dos.mod", b"uno.mod"]);
    assert_eq!(m.iter().find(|(n, _, _)| n == b"uno.mod").unwrap().2, 4, "el de Work, el primero");
    // El tope.
    assert_eq!(b.dos.listar_cajon(&mut b.caja, b"MOD:", 1).unwrap().len(), 1);
    // Los dispositivos: los volúmenes primero, y la asignación.
    let d = b.dos.nombres_de_dispositivos();
    assert_eq!(d[0], (b"SYS".to_vec(), true));
    assert!(d.iter().any(|(n, v)| n == b"MOD" && !v));
}

#[test]
fn listar_hostil() {
    let mut b = Banco::nuevo();
    // Lo que no existe, un fichero, nombres malos o un bucle: su error de dos, sin pánico.
    assert_eq!(b.dos.listar_cajon(&mut b.caja, b"Work:NoEsta", 10), Err(205));
    assert_eq!(b.dos.listar_cajon(&mut b.caja, b"SYS:S/Startup-Sequence", 10), Err(212));
    assert!(b.dos.listar_cajon(&mut b.caja, b"NoHay:", 10).is_err());
    assert!(b.dos.listar_cajon(&mut b.caja, &[b'a'; 5000], 10).is_err());
    b.dos.asignar_tarde(&mut b.caja, "BUCLE", "BUCLE:").unwrap();
    assert!(b.dos.listar_cajon(&mut b.caja, b"BUCLE:", 10).is_err());
    b.limpio(0, 1);
}
