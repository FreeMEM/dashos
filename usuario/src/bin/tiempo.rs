//! C49: temporizadores. Una vez y cada cierto tiempo, que se paran al cerrar la llave, y los casos que se rechazan.
//! Sale con 49 si todo va bien, o con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{ahora_us, canal_crear, canal_escribir, canal_leer, cerrar, llave_duplicar, println, temporizador_crear, LEER};

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
    let (n, _) = aviso(a);
    let pasado = ahora_us() - t0;
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
    println!("  [tiempo] una vez, periódico, se para al cerrar y rechaza lo que no vale");
    49
}
