//! C49: temporizadores. Una vez y cada cierto tiempo, que se paran al cerrar la llave, y los casos que se rechazan.
//! Sale con 49 si todo va bien, o con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{ahora_us, canal_crear, canal_escribir, canal_leer, canal_leer_marca, cerrar, llave_duplicar, llave_revocable, println, revocar,
    temporizador_crear, ESCRIBIR, LEER};

dashos::programa!(principal);

const E_ARGUMENTO: i64 = -1;
const E_LLAVE: i64 = -2;
const E_DERECHOS: i64 = -3;
const E_TIPO: i64 = -6;

/// Espera el siguiente aviso en `a`: (bytes, cuenta).
fn aviso(a: u32) -> (i64, u64) {
    let mut buf = [0u8; 16];
    let mut ll = [0u32; 1];
    let (n, _) = canal_leer(a, &mut buf, &mut ll);
    (n, u64::from_le_bytes(buf[..8].try_into().unwrap()))
}

fn principal() -> u64 {
    let (a, b) = canal_crear();
    // 1-2: una vez, a los 50 ms: llega un aviso de 8 bytes con la cuenta 1, y no antes de tiempo.
    let t0 = ahora_us();
    let t = temporizador_crear(b, 50_000, 0);
    if t <= 0 {
        return 1;
    }
    let (n, c) = aviso(a);
    let pasado = ahora_us() - t0;
    if n != 8 || c != 1 || pasado < 50_000 || pasado > 500_000 {
        println!("  [tiempo] una vez: {} bytes, cuenta {}, {} µs", n, c, pasado);
        return 2;
    }
    cerrar(t as u32);
    // 3: cada 20 ms: cinco avisos que suman al menos 5 vencimientos, en no menos de 80 ms.
    let t0 = ahora_us();
    let p = temporizador_crear(b, 20_000, 20_000);
    let mut suma = 0;
    for _ in 0..5 {
        let (n, c) = aviso(a);
        if n != 8 {
            return 3;
        }
        suma += c;
    }
    let pasado = ahora_us() - t0;
    if p <= 0 || suma < 5 || pasado < 80_000 {
        println!("  [tiempo] periódico: suma {}, {} µs", suma, pasado);
        return 3;
    }
    // 4: al cerrar su llave se para. Lo siguiente que llega es el de otro de 100 ms, no uno del periódico (20 ms).
    cerrar(p as u32);
    let t0 = ahora_us();
    let otro = temporizador_crear(b, 100_000, 0);
    let (mut n, _) = aviso(a);
    let mut pasado = ahora_us() - t0;
    // Puede llegar uno más del periódico, si ya estaba saliendo al cerrarlo (kernel.md, B10): uno, no más.
    if pasado < 90_000 {
        n = aviso(a).0;
        pasado = ahora_us() - t0;
    }
    if otro <= 0 || n != 8 || pasado < 90_000 {
        println!("  [tiempo] tras cerrar el periódico llegó algo a los {} µs", pasado);
        return 4;
    }
    cerrar(otro as u32);
    // 5-8: lo que se rechaza: una llave que no es de canal, una que no existe, un canal sin derecho de escribir y un
    // plazo de más de un día.
    if temporizador_crear(otro as u32, 1000, 0) != E_LLAVE {
        return 5;
    }
    let solo_leer = llave_duplicar(b, LEER, 0);
    let (c1, _c2) = canal_crear();
    let tm = temporizador_crear(c1, 1_000_000, 0);
    if temporizador_crear(tm as u32, 1000, 0) != E_TIPO {
        return 6;
    }
    if solo_leer <= 0 || temporizador_crear(solo_leer as u32, 1000, 0) != E_DERECHOS {
        return 7;
    }
    if temporizador_crear(b, 86_400_000_001, 0) != E_ARGUMENTO {
        return 8;
    }
    // 9: un temporizador no puede ir por el canal al que avisa (sería un ciclo que no se suelta nunca).
    if canal_escribir(c1, b"t", &[tm as u32]) >= 0 {
        return 9;
    }
    // 10 (auditoría de la fase 4, A1): el periodo más corto es un tic. Uno de 1 µs, en tres avisos, no pasa de unas
    // decenas de vencimientos (sin el tope serían decenas de miles).
    let (a2, b2) = canal_crear();
    let r = temporizador_crear(b2, 1, 1);
    let mut suma = 0;
    for _ in 0..3 {
        suma += aviso(a2).1;
    }
    cerrar(r as u32);
    if suma > 30 {
        println!("  [tiempo] periodo de 1 µs: {} vencimientos en tres avisos", suma);
        return 10;
    }
    // 11-12 (A2): los avisos llevan la marca de la llave del canal, y revocar esa llave para el temporizador.
    let (a3, b3) = canal_crear();
    let marcada = llave_duplicar(b3, ESCRIBIR, 77);
    let tm = temporizador_crear(marcada as u32, 10_000, 0);
    let mut buf = [0u8; 16];
    let mut ll = [0u32; 1];
    let (n, _, marca) = canal_leer_marca(a3, &mut buf, &mut ll);
    cerrar(tm as u32);
    if n != 8 || marca != 77 {
        println!("  [tiempo] aviso con marca {}", marca);
        return 11;
    }
    let (copia, revocador) = llave_revocable(b3);
    let p = temporizador_crear(copia as u32, 20_000, 20_000);
    aviso(a3);
    revocar(revocador);
    let t0 = ahora_us();
    let otro = temporizador_crear(b3, 100_000, 0);
    let pasado = {
        aviso(a3);
        ahora_us() - t0
    };
    cerrar(p as u32);
    cerrar(otro as u32);
    if p <= 0 || pasado < 90_000 {
        println!("  [tiempo] tras revocar, un aviso a los {} µs", pasado);
        return 12;
    }
    // 13 (M1): uno de una vez con la cola llena no se pierde: llega cuando hay sitio. Un vigilante de 500 ms con otra
    // marca dice si se perdió.
    let (a4, b4) = canal_crear();
    for _ in 0..16 {
        canal_escribir(b4, b"x", &[]);
    }
    let plazo = temporizador_crear(llave_duplicar(b4, ESCRIBIR, 1) as u32, 20_000, 0);
    let t0 = ahora_us();
    while ahora_us() - t0 < 60_000 {}
    for _ in 0..16 {
        canal_leer(a4, &mut buf, &mut ll);
    }
    let vigilante = temporizador_crear(llave_duplicar(b4, ESCRIBIR, 2) as u32, 500_000, 0);
    let (n, _, marca) = canal_leer_marca(a4, &mut buf, &mut ll);
    cerrar(plazo as u32);
    cerrar(vigilante as u32);
    if n != 8 || marca != 1 {
        println!("  [tiempo] con la cola llena, el aviso de una vez se perdió (llegó la marca {})", marca);
        return 13;
    }
    // 14 (segunda vuelta de la auditoría, C1): una cadena de temporizadores, cada uno con una llave revocable del
    // lado de un canal en cuya cola va el siguiente. Al soltar el primero se suelta la cadena entera: sin recursión en
    // la pila del kernel.
    let mut anterior: Option<u32> = None;
    let mut primero = 0;
    for i in 0..30 {
        let (a, b) = canal_crear();
        let (ra, rev) = llave_revocable(a);
        cerrar(a);
        let t = temporizador_crear(ra as u32, 86_400_000_000, 0);
        cerrar(ra as u32);
        cerrar(rev);
        if t <= 0 {
            return 14;
        }
        match anterior {
            Some(b_ant) => {
                canal_escribir(b_ant, b"t", &[t as u32]);
                cerrar(b_ant);
            }
            None => primero = t as u32,
        }
        anterior = Some(b);
        let _ = i;
    }
    if let Some(b) = anterior {
        cerrar(b);
    }
    cerrar(primero);
    println!("  [tiempo] una vez, periódico, se para al cerrar y rechaza lo que no vale; periodo mínimo, marca, revocación y cola llena; cadena de 30 soltada");
    49
}
