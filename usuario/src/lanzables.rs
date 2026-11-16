//! S10: lo que la barra puede lanzar por `sesiones`, y quién es de verdad un programa del sistema. Lo usan `sesiones`
//! (que decide) y la barra (que solo enseña lo que se puede lanzar). Sin `alloc`: se prueba en el PC con
//! `rustc --edition 2024 --test usuario/src/lanzables.rs`.

/// El cajón de donde lanza la barra.
pub const CAJON: &[u8] = b"Programas/";

/// Lo más largo que puede ser el nombre de un programa que lanza la barra.
pub const MAX_NOMBRE: usize = 30;

/// Lo que lanza `sesiones` se numera desde aquí; lo que lanza `inicio`, por debajo (su número en la lista más 1).
pub const PRIMER_PID_PROPIO: u64 = 0x1_0000;

/// Si `nombre` vale como programa de `Programas/`: no vacío, corto, sin cajones ni subir, y sin letras de control.
pub fn nombre_valido(nombre: &[u8]) -> bool {
    !nombre.is_empty()
        && nombre.len() <= MAX_NOMBRE
        && nombre != b"."
        && nombre != b".."
        && !nombre.iter().any(|&c| c == b'/' || c == b':' || c < 0x20 || c == 0x7f)
}

/// Si `camino` es un programa de `Programas/`: lo único que la barra puede lanzar (S10).
pub fn de_programas(camino: &[u8]) -> bool {
    camino.strip_prefix(CAJON).is_some_and(nombre_valido)
}

/// El Shell nativo (`docs/bitacoras/shell-nativa.md`): está en `SYS:System`, no en `Programas/`, y el lanzador lo
/// enseña con este nombre.
pub const SHELL: &[u8] = b"System/Shell";
pub const NOMBRE_SHELL: &str = "Shell";

/// Si la barra (y el escritorio) puede pedir que se lance `camino`: un programa de `Programas/`, o el Shell.
pub fn lanzable(camino: &[u8]) -> bool {
    de_programas(camino) || camino == SHELL
}

/// Si el proceso de la marca `marca`, que se llama `su_nombre`, es uno de los programas del sistema `nombres`. Solo
/// cuenta el nombre que da `inicio` (marca baja): lo lanzado desde el disco se llama como su fichero, y un fichero
/// `Programas/guardian` no es `guardian`.
pub fn del_sistema(marca: u64, su_nombre: &str, nombres: &[&str]) -> bool {
    marca < PRIMER_PID_PROPIO && nombres.contains(&su_nombre)
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn solo_programas_de_su_cajon() {
        assert!(de_programas(b"Programas/hola"));
        // El Shell es lo único de fuera de `Programas/` que se deja lanzar; lo que se le parece, no.
        assert!(lanzable(b"System/Shell") && lanzable(b"Programas/hola"));
        for malo in [&b"System/Shell2"[..], b"System/shell", b"System/Format", b"System/", b"/System/Shell", b"System/../System/Shell", b"Programas/../System/Shell"] {
            assert!(!lanzable(malo), "{:?}", core::str::from_utf8(malo));
        }
        assert!(de_programas(b"Programas/Calculadora 2"));
        for malo in [
            &b"Programas/"[..],
            b"Programas/.",
            b"Programas/..",
            b"Programas/../C/Privado/nota",
            b"Programas/sub/hola",
            b"Programas/a:b",
            b"Programas/ho\nla",
            b"Programas/ho\x7fla",
            b"C/Privado/nota",
            b"LEEME.txt",
            b"programas/hola",
            b"/Programas/hola",
            b"",
        ] {
            assert!(!de_programas(malo), "{:?}", core::str::from_utf8(malo));
        }
        let largo = [b'a'; MAX_NOMBRE + 1];
        let mut camino = [0u8; 10 + MAX_NOMBRE + 1];
        camino[..10].copy_from_slice(CAJON);
        camino[10..].copy_from_slice(&largo);
        assert!(!de_programas(&camino));
        assert!(de_programas(&camino[..camino.len() - 1]));
    }

    #[test]
    fn el_nombre_de_un_fichero_no_da_poderes() {
        let guardianes = &["guardian", "entrada"];
        assert!(del_sistema(12, "guardian", guardianes));
        assert!(!del_sistema(12, "hola", guardianes));
        // Lanzado desde el disco con el nombre de un programa del sistema: no lo es.
        assert!(!del_sistema(PRIMER_PID_PROPIO, "guardian", guardianes));
        assert!(!del_sistema(PRIMER_PID_PROPIO + 7, "entrada", guardianes));
    }
}
