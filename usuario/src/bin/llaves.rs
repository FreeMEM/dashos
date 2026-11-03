//! C41: llaves completas. Duplicar con menos derechos, marcar quién escribe y revocar. Sale con 41 si todo va bien, o
//! con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{canal_crear, canal_escribir, canal_leer, canal_leer_marca, llamada, llave_duplicar, llave_revocable, println, revocar,
    ESCRIBIR, LEER, TRANSFERIR};

dashos::programa!(principal);

const E_LLAVE: i64 = -2;
const E_DERECHOS: i64 = -3;
const E_CERRADO: i64 = -4;
const E_ARGUMENTO: i64 = -1;
const E_LIMITE: i64 = -8;

fn cerrar(k: u32) {
    llamada(6, [k as u64, 0, 0, 0, 0]);
}

fn principal() -> u64 {
    let mut buf = [0u8; 8];
    let mut ll = [0u32; 2];
    let (a, b) = canal_crear();
    // 1-2: una copia solo para escribir: escribe, pero no lee.
    let a2 = llave_duplicar(a, ESCRIBIR, 0);
    if a2 <= 0 || canal_escribir(a2 as u32, b"1", &[]) != 0 || canal_leer(b, &mut buf, &mut ll).0 != 1 {
        return 1;
    }
    if canal_leer(a2 as u32, &mut buf, &mut ll).0 != E_DERECHOS {
        return 2;
    }
    // 3: pedir más derechos al duplicar no los da.
    let a3 = llave_duplicar(a2 as u32, LEER | ESCRIBIR | TRANSFERIR, 0);
    if a3 <= 0 || canal_leer(a3 as u32, &mut buf, &mut ll).0 != E_DERECHOS {
        return 3;
    }
    // 4-5: la marca llega con el mensaje; sin marca, 0.
    let am = llave_duplicar(a, ESCRIBIR, 42);
    if am <= 0 || canal_escribir(am as u32, b"m", &[]) != 0 || canal_leer_marca(b, &mut buf, &mut ll) != (1, 0, 42) {
        return 4;
    }
    if canal_escribir(a, b"s", &[]) != 0 || canal_leer_marca(b, &mut buf, &mut ll) != (1, 0, 0) {
        return 5;
    }
    // 6-7: una llave marcada no se vuelve a marcar con otra; duplicarla con 0 conserva la suya.
    if llave_duplicar(am as u32, ESCRIBIR, 7) != E_DERECHOS {
        return 6;
    }
    let am2 = llave_duplicar(am as u32, ESCRIBIR, 0);
    if am2 <= 0 || canal_escribir(am2 as u32, b"m", &[]) != 0 || canal_leer_marca(b, &mut buf, &mut ll) != (1, 0, 42) {
        return 7;
    }
    // 8-10: revocar. La copia revocable vale hasta que se revoca; después, ni ella ni sus copias.
    let (r, rev) = llave_revocable(a);
    if r <= 0 || canal_escribir(r as u32, b"r", &[]) != 0 || canal_leer(b, &mut buf, &mut ll).0 != 1 {
        return 8;
    }
    let r2 = llave_duplicar(r as u32, ESCRIBIR, 0);
    if r2 <= 0 || revocar(rev) != 0 {
        return 9;
    }
    if canal_escribir(r as u32, b"r", &[]) != E_LLAVE || canal_escribir(r2 as u32, b"r", &[]) != E_LLAVE {
        return 10;
    }
    // 11: el original sigue valiendo.
    if canal_escribir(a, b"o", &[]) != 0 || canal_leer(b, &mut buf, &mut ll).0 != 1 {
        return 11;
    }
    // 12: con las copias, un lado sigue abierto mientras quede una; se cierra con la última.
    let (e, f) = canal_crear();
    let e2 = llave_duplicar(e, ESCRIBIR, 0);
    cerrar(e);
    if canal_escribir(e2 as u32, b"e", &[]) != 0 || canal_leer(f, &mut buf, &mut ll).0 != 1 {
        return 12;
    }
    cerrar(e2 as u32);
    if canal_leer(f, &mut buf, &mut ll).0 != E_CERRADO {
        return 13;
    }
    // 14: revocar suelta el objeto: si la copia revocable era lo último de un lado, el lado se cierra.
    let (g, h) = canal_crear();
    let (rg, revg) = llave_revocable(g);
    cerrar(g);
    if rg <= 0 || revocar(revg) != 0 || canal_leer(h, &mut buf, &mut ll).0 != E_CERRADO {
        return 14;
    }
    // 15: una cadena de revocables tiene fondo (auditoría de la fase 3, C1): a la quinta, E_LIMITE. Lo mismo con
    // revocadores de revocadores.
    let m = dashos::memoria_crear(4096);
    let mut h = m;
    let mut hechas = 0;
    let fin = loop {
        let (c, rev) = llave_revocable(h);
        if c < 0 {
            break c;
        }
        cerrar(rev);
        h = c as u32;
        hechas += 1;
    };
    if fin != E_LIMITE || hechas != 4 {
        return 15;
    }
    let (_, mut rev) = llave_revocable(m);
    let mut hechas = 1;
    loop {
        let (c, r) = llave_revocable(rev);
        if c < 0 {
            if c != E_LIMITE || hechas != 4 {
                return 16;
            }
            break;
        }
        rev = r;
        hechas += 1;
    }
    // 17: un revocador cuyo objeto es un lado del mismo canal también es un ciclo (M1).
    let (x, y) = canal_crear();
    let (_, revy) = llave_revocable(y);
    if canal_escribir(x, b"c", &[revy]) != E_ARGUMENTO {
        return 17;
    }
    println!("  [llaves] copias con menos derechos, marcas que no se cambian y revocación");
    41
}
