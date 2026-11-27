//! ParsePattern, MatchPattern (y NoCase), ExAll y ExAllEnd.

mod comun;
use comun::*;

use caja_dos::errores::*;
use caja_dos::estructuras::*;
use caja_dos::exall::{eac, ed};
use caja_dos::*;

fn casa(b: &mut Banco, patron: &str, texto: &str, nocase: bool) -> bool {
    let p = b.cad(patron);
    let buf = b.hueco(2 * patron.len() as u32 + 2);
    let r = b.llamar(
        if nocase { lvo::PARSE_PATTERN_NO_CASE } else { lvo::PARSE_PATTERN },
        &[p, buf, 2 * patron.len() as u32 + 2],
    );
    assert_ne!(r as i32, -1, "{patron}");
    let t = b.cad(texto);
    let m = b.llamar(if nocase { lvo::MATCH_PATTERN_NO_CASE } else { lvo::MATCH_PATTERN }, &[buf, t]);
    m == DOSTRUE
}

#[test]
fn patrones() {
    let mut b = Banco::nuevo();
    for (p, t, si) in [
        ("#?", "", true),
        ("#?", "lo que sea", true),
        ("#?.info", "Disco.info", true),
        ("#?.info", "Disco.inf", false),
        ("?", "a", true),
        ("?", "ab", false),
        ("a#b", "a", true),
        ("a#b", "abbb", true),
        ("a#b", "abc", false),
        ("(uno|dos)", "dos", true),
        ("(uno|dos)", "tres", false),
        ("~(#?.info)", "fichero", true),
        ("~(#?.info)", "fichero.info", false),
        ("[a-c]x", "bx", true),
        ("[a-c]x", "dx", false),
        ("[~a-c]x", "dx", true),
        ("[~a-c]x", "ax", false),
        ("a%b", "ab", true),
        ("'#?", "#?", true),
        ("'#?", "x", false),
        ("#(ab)", "ababab", true),
        ("#(ab)", "aba", false),
        ("#?(.c|.h)", "main.c", true),
        ("*", "*", true),
        ("*", "x", false),
    ] {
        assert_eq!(casa(&mut b, p, t, false), si, "{p} con {t}");
    }
    // Mayúsculas.
    assert!(!casa(&mut b, "#?.INFO", "x.info", false));
    assert!(casa(&mut b, "#?.INFO", "x.info", true));
    assert!(casa(&mut b, "[a-c]", "B", true));
    assert!(casa(&mut b, "ñ", "Ñ", true));
    // Si lleva comodines o no.
    let buf = b.hueco(64);
    for (p, r) in [("hola", 0), ("ho#?la", 1), ("h'?", 0), ("a|b", 1), ("(a", -1), ("[a", -1), ("#", -1), ("'", -1)] {
        let pp = b.cad(p);
        assert_eq!(b.llamar(lvo::PARSE_PATTERN, &[pp, buf, 64]) as i32, r, "{p}");
    }
    // Un búfer corto.
    let pp = b.cad("#?.info");
    assert_eq!(b.llamar(lvo::PARSE_PATTERN, &[pp, buf, 4]) as i32, -1);
    // Patrones hechos para atascar: acaban pronto, o no se aceptan por largos.
    let texto = "a".repeat(250);
    assert!(!casa(&mut b, &("#".repeat(100) + "?x"), &texto, false));
    assert!(!casa(&mut b, &("#(#?#?#?#?)".repeat(4) + "x"), &texto, true));
    assert!(!casa(&mut b, &"#?~(#?)".repeat(12), &texto, false));
    let pp = b.cad(&("#".repeat(200) + "?x"));
    assert_eq!(b.llamar(lvo::PARSE_PATTERN, &[pp, buf, 64]) as i32, -1);
    // Un texto de más de 256: no casa (MatchPattern con un patrón sin analizar, como hacen algunos programas).
    let largo = b.cad(&"a".repeat(300));
    let todo = b.cad("#?");
    assert_eq!(b.llamar(lvo::MATCH_PATTERN, &[todo, largo]), 0);
}

#[test]
fn exall() {
    let mut b = Banco::nuevo();
    for i in 0..30 {
        b.dos.fs.fs.poner(b.ram, &format!("d/fichero{i:02}{}", if i % 3 == 0 { ".info" } else { "" }), &vec![1; i]);
    }
    b.dos.fs.fs.poner_cajon(b.ram, "d/sub");
    let l = b.lock("RAM:d", SHARED_LOCK);
    let ctl = b.llamar(lvo::ALLOC_DOS_OBJECT, &[dos_obj::DOS_EXALLCONTROL, 0]);
    // Un búfer pequeño: varias vueltas.
    let buf = b.hueco(300);
    let mut nombres = Vec::new();
    loop {
        let mas = b.llamar(lvo::EXALL, &[l, buf, 300, ed::TIPO_SIZE, ctl]);
        let n = b.l32(ctl + eac::EAC_ENTRIES);
        let mut e = if n > 0 { buf } else { 0 };
        let mut k = 0;
        while e != 0 {
            let nombre = b.cadena_en(b.l32(e + ed::ED_NAME));
            let tamano = b.l32(e + ed::ED_SIZE);
            let tipo = b.l32(e + ed::ED_TYPE) as i32;
            nombres.push((nombre, tamano, tipo));
            e = b.l32(e + ed::ED_NEXT);
            k += 1;
        }
        assert_eq!(k, n);
        if mas == 0 {
            assert_eq!(b.io_err(), ERROR_NO_MORE_ENTRIES);
            break;
        }
    }
    assert_eq!(nombres.len(), 31);
    assert_eq!(nombres[5], ("fichero05".to_string(), 5, st::FILE));
    assert_eq!(nombres[30].0, "sub");
    assert_eq!(nombres[30].2, st::USERDIR);
    // Con patrón: solo los .info.
    let pat = b.cad("#?.INFO");
    let pbuf = b.hueco(32);
    b.llamar(lvo::PARSE_PATTERN_NO_CASE, &[pat, pbuf, 32]);
    b.caja.mem.escribir32(ctl + eac::EAC_MATCHSTRING, pbuf);
    let grande = b.hueco(4096);
    assert_eq!(b.llamar(lvo::EXALL, &[l, grande, 4096, ed::TIPO_OWNER, ctl]), 0);
    assert_eq!(b.io_err(), ERROR_NO_MORE_ENTRIES);
    assert_eq!(b.l32(ctl + eac::EAC_ENTRIES), 10);
    assert_eq!(b.cadena_en(b.l32(grande + ed::ED_NAME)), "fichero00.info");
    assert_eq!(b.cadena_en(b.l32(grande + ed::ED_COMMENT)), "");
    // Tipo que no hay, búfer que no da ni para una, sobre un fichero.
    assert_eq!(b.llamar(lvo::EXALL, &[l, grande, 4096, 8, ctl]), 0);
    assert_eq!(b.io_err(), ERROR_BAD_NUMBER);
    b.caja.mem.escribir32(ctl + eac::EAC_MATCHSTRING, 0);
    assert_eq!(b.llamar(lvo::EXALL, &[l, grande, 10, ed::TIPO_NAME, ctl]), 0);
    assert_eq!(b.io_err(), ERROR_NO_FREE_STORE);
    let f = b.lock("RAM:d/fichero01", SHARED_LOCK);
    assert_eq!(b.llamar(lvo::EXALL, &[f, grande, 4096, ed::TIPO_NAME, ctl]), 0);
    assert_eq!(b.io_err(), ERROR_OBJECT_WRONG_TYPE);
    // ExAllEnd a medias.
    b.llamar(lvo::EXALL, &[l, buf, 100, ed::TIPO_NAME, ctl]);
    assert_ne!(b.l32(ctl + eac::EAC_LASTKEY), 0);
    b.llamar(lvo::EXALL_END, &[l, buf, 100, ed::TIPO_NAME, ctl]);
    assert_eq!(b.l32(ctl + eac::EAC_LASTKEY), 0);
    b.unlock(f);
    b.unlock(l);
    b.llamar(lvo::FREE_DOS_OBJECT, &[dos_obj::DOS_EXALLCONTROL, ctl]);
    b.limpio(0, 1);
}
