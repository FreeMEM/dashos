//! C42: cupos por grupo. Llave 1: el ELF de `hijo` en un objeto de memoria; llave 2: un canal con su tamaño. Sale con
//! 42 si todo va bien, o con el número del paso que falló.

#![no_std]
#![no_main]

use dashos::{canal_crear, canal_escribir, grupo_crear, grupo_matar, grupo_uso, println, proceso_crear_en, proceso_esperar};

fn esperar_ms(ms: u64) {
    let (t, f): (u64, u64);
    unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", "mrs {}, cntfrq_el0", out(reg) t, out(reg) f) };
    let fin = t + f * ms / 1000;
    loop {
        let a: u64;
        unsafe { core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) a) };
        if a >= fin {
            return;
        }
    }
}

dashos::programa!(principal);

const MB: u64 = 1 << 20;

fn hijo_en(grupo: u32, tamano: usize, orden: &[u8]) -> i64 {
    let (a, b) = canal_crear();
    if !orden.is_empty() && canal_escribir(a, orden, &[]) != 0 {
        return -100;
    }
    proceso_crear_en(grupo, 1, tamano, &[b])
}

fn principal() -> u64 {
    let mut buf = [0u8; 8];
    let mut llaves = [0u32; 1];
    if dashos::canal_leer(2, &mut buf, &mut llaves).0 != 8 {
        return 1;
    }
    let tamano = u64::from_le_bytes(buf) as usize;
    // 2: un grupo de 1 MB, vacío al empezar.
    let g = grupo_crear(MB);
    if g <= 0 || grupo_uso(g as u32) != (MB as i64, 0) {
        return 2;
    }
    let g = g as u32;
    // 3: un hijo que se come la memoria dentro del grupo: le dan algo, menos de 1 MB, y acaba en E_MEMORIA.
    let p = hijo_en(g, tamano, &[4]);
    if p <= 0 {
        return 3;
    }
    let (clase, trozos) = proceso_esperar(p as u32);
    if clase != 0 || trozos == 0 || trozos >= 16 {
        return 4;
    }
    // 5: al acabar el hijo, todo lo cobrado vuelve al grupo (cuando su núcleo lo recoge: ver el paso 13).
    if !vuelve_a_cero(g) {
        return 5;
    }
    // 6 (S3, cupos como topes): un grupo de 2 MB; su proceso crea dentro un subgrupo de 64 MB, más que el suyo, y un
    // nieto ahí se come la memoria: lo para el tope de 2 MB de arriba, no el de 64. Al acabar, todo vuelve en la cadena.
    let gp = grupo_crear(2 * MB);
    if gp <= 0 {
        return 6;
    }
    let trozos_nieto = con_nieto(gp as u32, tamano, 5);
    if !(1..32).contains(&trozos_nieto) || !vuelve_a_cero(gp as u32) {
        println!("  [grupos] nieto bajo un tope de 2 MB: {} trozos de 64 KB", trozos_nieto);
        return 6;
    }
    // 7-9: matar un grupo mata a todos sus procesos.
    let g2 = grupo_crear(4 * MB);
    if g2 <= 0 {
        return 7;
    }
    let p1 = hijo_en(g2 as u32, tamano, &[]);
    let p2 = hijo_en(g2 as u32, tamano, &[]);
    if p1 <= 0 || p2 <= 0 || grupo_matar(g2 as u32) != 0 {
        return 8;
    }
    if proceso_esperar(p1 as u32).0 != 1 || proceso_esperar(p2 as u32).0 != 1 {
        return 9;
    }
    // 10: el propio grupo no se ha tocado: aquí aún caben 4 MB.
    if dashos::memoria_crear(4 * MB as usize) as i32 <= 0 {
        return 10;
    }
    // C44. 11-12: un hijo deja un ciclo de dos canales y sale. Lo del ciclo sigue cobrado a su grupo; al matar el
    // grupo, sus canales se cierran y el grupo vuelve a 0.
    let g3 = grupo_crear(MB);
    if g3 <= 0 {
        return 11;
    }
    let p = hijo_en(g3 as u32, tamano, &[6]);
    if p <= 0 || proceso_esperar(p as u32) != (0, 0) || grupo_uso(g3 as u32).1 == 0 {
        return 12;
    }
    // Lo del hijo, que ya acabó, se devuelve cuando su núcleo recoge sus restos (`planificador::recoger`, en la
    // siguiente llamada o al quedar ocioso), no al acabar: se espera hasta 200 ms a que el grupo vuelva a 0. Antes se
    // miraba en el acto y, rara vez, el núcleo del hijo aún no lo había recogido.
    if grupo_matar(g3 as u32) != 0 {
        return 13;
    }
    if !vuelve_a_cero(g3 as u32) {
        return 13;
    }
    // 14: estrés de A2. Un proceso X gira; seis hijos, en otro grupo, esperan a X con copias de su llave. Se matan a
    // la vez los que esperan y X, diez veces. Si alguien despertara a un hilo ya liberado, el kernel caería.
    for _ in 0..10 {
        let gx = grupo_crear(MB);
        let gw = grupo_crear(2 * MB);
        if gx <= 0 || gw <= 0 {
            return 14;
        }
        let x = hijo_en(gx as u32, tamano, &[2]);
        if x <= 0 {
            return 15;
        }
        for _ in 0..6 {
            let (a, b) = canal_crear();
            let copia = dashos::llave_duplicar(x as u32, dashos::LEER | dashos::TRANSFERIR, 0);
            if canal_escribir(a, &[7], &[]) != 0 || proceso_crear_en(gw as u32, 1, tamano, &[b, copia as u32]) <= 0 {
                return 16;
            }
        }
        esperar_ms(5);
        grupo_matar(gw as u32);
        dashos::proceso_matar(x as u32);
        if proceso_esperar(x as u32).0 != 1 {
            return 17;
        }
        // Soltar las llaves de los grupos y de X: su presupuesto vuelve cuando acaben de morir todos.
        for k in [gx, gw, x] {
            dashos::llamada(6, [k as u64, 0, 0, 0, 0]);
        }
    }
    // 18: estrés de M2. Un hijo crea hijos sin parar en su grupo; se mata el grupo. Si alguno se escapara al nacer
    // durante la matanza, seguiría girando y el grupo no volvería nunca a 0.
    let g4 = grupo_crear(16 * MB);
    if g4 <= 0 {
        return 18;
    }
    let mut orden = [0u8; 9];
    orden[0] = 8;
    orden[1..9].copy_from_slice(&(tamano as u64).to_le_bytes());
    let (a, b) = canal_crear();
    let copia_elf = dashos::llave_duplicar(1, dashos::LEER | dashos::TRANSFERIR, 0);
    if canal_escribir(a, &orden, &[]) != 0 || proceso_crear_en(g4 as u32, 1, tamano, &[b, copia_elf as u32]) <= 0 {
        return 19;
    }
    esperar_ms(20);
    grupo_matar(g4 as u32);
    let mut vuelto = false;
    for _ in 0..200 {
        esperar_ms(10);
        if grupo_uso(g4 as u32).1 == 0 {
            vuelto = true;
            break;
        }
    }
    if !vuelto {
        return 20;
    }
    // 28-35 (S3): los cupos son topes.
    let (ok, lleno) = topes(tamano);
    if ok != 0 {
        return ok;
    }
    // 21-27, trasfondo: el tiempo de CPU por grupo (llamada 52) y los núcleos (llamada 29).
    let (ok, cpu_hijo) = tiempo_de_cpu(tamano);
    if ok != 0 {
        return ok;
    }
    // 40-59: memoria dispersa (llamadas 57 y 58), para el `RAM:` de una sesión.
    let (ok, puestas) = dispersa(tamano);
    if ok != 0 {
        return ok;
    }
    println!("  [grupos] cupo agotado en {} trozos de 64 KB, devuelto entero, y grupos que se matan", trozos);
    println!("  [grupos] ciclos soltados al matar el grupo, y matanzas a la vez sin caerse");
    println!("  [grupos] topes: el propio no se sube, uno por debajo de lo usado no quita nada y no deja cobrar, y el aviso de memoria pide su permiso");
    println!("  [grupos] topes: un nieto con tope de 64 MB bajo uno de 2 MB se para en {} KB; con la memoria llena ({} MB) falla limpio y todo vuelve", trozos_nieto * 64, lleno);
    println!("  [grupos] tiempo de CPU: el hijo que gira gastó {} ms en su grupo, cuenta también en el de arriba y no se pierde al matarlo", cpu_hijo / 1000);
    println!("  [grupos] memoria dispersa: vacía no cobra más que su lista, cada página se cobra al ponerla y vuelve al soltarla, el tope de su grupo la para en {} KB, un hueco es un fallo y lo ajeno o compartido no se toca", puestas * 4);
    42
}

/// Memoria dispersa (llamadas 57 y 58): un objeto de 4 MB en un grupo de 1 MB. El paso que falla y 0, o 0 y cuántas
/// páginas dejó poner el tope.
fn dispersa(tamano: usize) -> (u64, u64) {
    use dashos::{memoria_dispersa, memoria_paginas, memoria_proyectar};
    const VA: usize = 0x31_0000_0000;
    const PAGINAS: usize = 1024;
    let g = grupo_crear(MB);
    if g <= 0 {
        return (40, 0);
    }
    let g = g as u32;
    // 41: más grande que el tope de su grupo y nace igual: vacío solo cuesta su lista (256 bytes y 8 por página).
    let m = memoria_dispersa(PAGINAS * 4096, g);
    let fijo = grupo_uso(g).1;
    if m <= 0 || fijo != 256 + 8 * PAGINAS as u64 || dashos::memoria_tamano(m as u32) != (PAGINAS * 4096) as i64 {
        return (41, 0);
    }
    let m = m as u32;
    // 42: lo que no vale. Tamaño 0 o de más de 1 GB, un grupo que no es un grupo o sin escribir.
    let solo_leer = dashos::llave_duplicar(g, dashos::LEER, 0);
    if memoria_dispersa(0, g) != -5 || memoria_dispersa((1 << 30) + 1, g) != -5 || memoria_dispersa(4096, m) != -6 || solo_leer <= 0 || memoria_dispersa(4096, solo_leer as u32) != -3 {
        return (42, 0);
    }
    // 43: proyectada entera sin ninguna página; se ponen 16 desde la 8, se cobran y se escriben y leen.
    if memoria_proyectar(m, VA, true) != 0 || memoria_paginas(m, true, 8, 16) != 16 || grupo_uso(g).1 != fijo + 16 * 4096 {
        return (43, 0);
    }
    let pagina = |k: usize| (VA + k * 4096) as *mut u64;
    for k in 8..24 {
        unsafe { core::ptr::write_volatile(pagina(k), 0xD15C0 + k as u64) };
    }
    if (8..24).any(|k| unsafe { core::ptr::read_volatile(pagina(k)) } != 0xD15C0 + k as u64) {
        return (44, 0);
    }
    // 45: poner las que ya están no cobra otra vez ni las borra; soltar las que no están no hace nada.
    if memoria_paginas(m, true, 8, 16) != 16 || grupo_uso(g).1 != fijo + 16 * 4096 || unsafe { core::ptr::read_volatile(pagina(9)) } != 0xD15C0 + 9 || memoria_paginas(m, false, 100, 50) != 50 {
        return (45, 0);
    }
    // 46: soltadas, vuelven al grupo; puesta otra vez, la página llega a cero (no con lo que tenía).
    if memoria_paginas(m, false, 8, 16) != 16 || grupo_uso(g).1 != fijo || memoria_paginas(m, true, 9, 1) != 1 || unsafe { core::ptr::read_volatile(pagina(9)) } != 0 {
        return (46, 0);
    }
    // 47: el tope del grupo (1 MB) para el llenado: se ponen menos de las 256 de 1 MB, las del final no, y nada pasa
    // del tope. Con el tope lleno, poner una más da E_MEMORIA.
    let puestas = memoria_paginas(m, true, 256, 256);
    let usado = grupo_uso(g).1;
    if !(200..256).contains(&puestas) || usado > MB || usado != fijo + (puestas as u64 + 1) * 4096 || memoria_paginas(m, true, 600, 1) != -7 {
        return (47, 0);
    }
    // 48: tramos y llaves que no valen: fuera del objeto, 0 o más de 256 páginas, otra operación, una llave que no es
    // de memoria, una sin escribir.
    let sin_escribir = dashos::llave_duplicar(m, dashos::LEER | dashos::TRANSFERIR, 0);
    if memoria_paginas(m, true, PAGINAS - 1, 2) != -5 || memoria_paginas(m, true, PAGINAS, 1) != -5 || memoria_paginas(m, true, usize::MAX, 1) != -5
        || memoria_paginas(m, true, 0, 0) != -5 || memoria_paginas(m, false, 0, 257) != -5 || dashos::llamada(58, [m as u64, 2, 0, 1, 0]).0 as i64 != -1
        || memoria_paginas(g, false, 0, 1) != -6 || sin_escribir <= 0 || memoria_paginas(sin_escribir as u32, false, 9, 1) != -3
    {
        return (48, 0);
    }
    // 49: compartida no se toca. Una copia de la llave viaja en un mensaje (el canal la tiene): ni soltar ni poner.
    // Leído el mensaje y cerrada la copia, otra vez sí.
    let (a, b) = canal_crear();
    let mut ll = [0u32; 1];
    if canal_escribir(a, &[1], &[sin_escribir as u32]) != 0 || memoria_paginas(m, false, 9, 1) != -3 || memoria_paginas(m, true, 700, 1) != -3 {
        return (49, 0);
    }
    if dashos::canal_leer(b, &mut [0u8; 8], &mut ll).1 != 1 || dashos::cerrar(ll[0]) != 0 || memoria_paginas(m, false, 9, 1) != 1 {
        return (50, 0);
    }
    dashos::cerrar(a);
    dashos::cerrar(b);
    // 51: soltado lo que puso el llenado, el grupo vuelve a lo fijo. Y un hijo que lee un hueco de su objeto disperso
    // muere por fallo (clase 2), y lo suyo vuelve.
    if memoria_paginas(m, false, 256, 256) != 256 || grupo_uso(g).1 != fijo {
        return (51, 0);
    }
    let p = hijo_en(g, tamano, &[18]);
    if p <= 0 || proceso_esperar(p as u32).0 != 2 {
        return (51, 0);
    }
    // 52: quitada la proyección y cerradas las llaves, el grupo vuelve a cero: páginas puestas y lista.
    if dashos::memoria_quitar(VA) != 0 || dashos::cerrar(m) != 0 || dashos::cerrar(solo_leer as u32) != 0 || !vuelve_a_cero(g) {
        return (52, 0);
    }
    dashos::cerrar(g);
    (0, puestas as u64)
}

/// Lanza en `grupo` un hijo con la orden `orden` (5 o 13) y el ELF de `hijo`: crea su subgrupo de 64 MB y un nieto
/// ahí. Con 5, espera y da los trozos que le dieron al nieto; con 13, da el número del hijo (sigue vivo).
fn con_nieto(grupo: u32, tamano: usize, orden: u8) -> u64 {
    let mut m = [0u8; 9];
    m[0] = orden;
    m[1..9].copy_from_slice(&(tamano as u64).to_le_bytes());
    let (a, b) = canal_crear();
    let elf = dashos::llave_duplicar(1, dashos::LEER | dashos::TRANSFERIR, 0);
    if elf <= 0 || canal_escribir(a, &m, &[]) != 0 {
        return 0;
    }
    let p = proceso_crear_en(grupo, 1, tamano, &[b, elf as u32]);
    if p <= 0 {
        return 0;
    }
    if orden == 13 {
        return p as u64;
    }
    match proceso_esperar(p as u32) {
        (0, t) => t,
        _ => 0,
    }
}

/// S3, pruebas hostiles de los topes. 0 y los MB a los que llegó al llenar la memoria si va bien; si no, el número del
/// paso que falló.
fn topes(tamano: usize) -> (u64, u64) {
    use dashos::grupo_uso_propio;
    let ram = dashos::medir(5, 0).max(0) as u64 * 4096;
    // 28: veinte grupos con topes de media RAM cada uno: suman diez veces la máquina, y se crean todos, vacíos.
    let mut muchos = [0u32; 20];
    for g in muchos.iter_mut() {
        let x = grupo_crear(ram / 2);
        if x <= 0 || grupo_uso_propio(x as u32) != ((ram / 2) as i64, 0, 0) {
            println!("  [grupos] grupo de {} MB: {}", ram >> 21, x);
            return (28, 0);
        }
        *g = x as u32;
    }
    // 29: uno de ellos gasta (8 objetos de 4 MB): le dan lo que pide, aunque los topes de todos pasen de la RAM.
    let p = hijo_en(muchos[7], tamano, &[11, 8]);
    if p <= 0 || proceso_esperar(p as u32) != (0, 0) {
        return (29, 0);
    }
    for g in muchos {
        dashos::cerrar(g);
    }
    // 30-31: un grupo de 2 MB con un subgrupo de 64 MB donde un nieto se come la memoria y se queda girando. Lo usado
    // por el grupo cuenta lo del subgrupo; lo propio, no (solo el hijo y el subgrupo en sí).
    let gp = grupo_crear(2 * MB);
    if gp <= 0 {
        return (30, 0);
    }
    let gp = gp as u32;
    let hijo = con_nieto(gp, tamano, 13);
    if hijo == 0 {
        return (30, 0);
    }
    let mut lleno_arriba = false;
    for _ in 0..500 {
        let (tope, usado, propio) = grupo_uso_propio(gp);
        if tope == 2 * MB as i64 && usado > 3 * MB / 2 && usado <= 2 * MB && propio < usado - MB {
            lleno_arriba = true;
            break;
        }
        dashos::dormir_us(2000);
    }
    if !lleno_arriba {
        println!("  [grupos] grupo con nieto que come: {:?}", grupo_uso_propio(gp));
        return (31, 0);
    }
    // 32: matar el grupo devuelve todo, también lo del nieto en el subgrupo.
    if grupo_matar(gp) != 0 || proceso_esperar(hijo as u32).0 != 1 || !vuelve_a_cero(gp) {
        println!("  [grupos] tras matar: {:?}", grupo_uso_propio(gp));
        return (32, 0);
    }
    // 33-34: llenar la memoria de verdad. Un grupo con un tope de diez veces la RAM; un hijo pide 16 MB tras otro hasta
    // que no le dan más: lo para la memoria que queda, no su tope, y falla limpio (E_MEMORIA, sin caerse nada).
    let g = grupo_crear(10 * ram);
    if g <= 0 {
        return (33, 0);
    }
    let g = g as u32;
    let p = hijo_en(g, tamano, &[15]);
    let (clase, trozos) = if p > 0 { proceso_esperar(p as u32) } else { (-1, 0) };
    if clase != 0 || trozos == 0 || trozos >= 250 || trozos * 16 * MB >= 10 * ram {
        println!("  [grupos] llenar la memoria: clase {}, {} trozos de 16 MB", clase, trozos);
        return (33, 0);
    }
    // 34-35: soltado, el grupo vuelve a cero y el sistema sigue: aquí caben otra vez 16 MB.
    if !vuelve_a_cero(g) {
        return (34, 0);
    }
    let m = dashos::memoria_crear((16 * MB) as usize);
    if (m as i32) <= 0 {
        return (35, 0);
    }
    dashos::cerrar(m);
    dashos::cerrar(g);
    // 36: un proceso no se sube su propio tope: con la llave de su grupo (con escribir), la llamada 54 da E_DERECHOS.
    let gs = grupo_crear(MB);
    if gs <= 0 {
        return (36, 0);
    }
    let gs = gs as u32;
    let copia = dashos::llave_duplicar(gs, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
    let (a, b) = canal_crear();
    let p = if copia > 0 && canal_escribir(a, &[16], &[]) == 0 { proceso_crear_en(gs, 1, tamano, &[b, copia as u32]) } else { -1 };
    if p <= 0 || proceso_esperar(p as u32) != (0, 3) || grupo_uso(gs).0 != MB as i64 {
        return (36, 0);
    }
    // 37: bajar el tope por debajo de lo usado. Vale, no se quita nada, y no se cobra más (un proceso nuevo no cabe)
    // hasta subirlo otra vez. Sin escribir, no se cambia.
    let gl = grupo_crear(4 * MB);
    if gl <= 0 {
        return (37, 0);
    }
    let gl = gl as u32;
    let comilon = hijo_en(gl, tamano, &[14]);
    let mut lleno = false;
    for _ in 0..500 {
        if grupo_uso(gl).1 > 3 * MB {
            lleno = true;
            break;
        }
        dashos::dormir_us(2000);
    }
    let sin_escribir = dashos::llave_duplicar(gl, dashos::LEER, 0);
    if comilon <= 0 || !lleno || sin_escribir <= 0 || dashos::grupo_tope(sin_escribir as u32, 64 * MB) != -3 {
        return (37, 0);
    }
    if dashos::grupo_tope(gl, MB) != 0 || grupo_uso(gl).0 != MB as i64 || grupo_uso(gl).1 <= 3 * MB {
        println!("  [grupos] tope bajado: {:?}", grupo_uso(gl));
        return (38, 0);
    }
    let otro = hijo_en(gl, tamano, &[1, 0]);
    if otro != -7 || dashos::grupo_tope(gl, 8 * MB) != 0 || grupo_matar(gl) != 0 || !vuelve_a_cero(gl) {
        println!("  [grupos] con el tope bajado, un proceso más: {}", otro);
        return (39, 0);
    }
    // 40: el aviso de memoria baja (llamada 53) pide su permiso: con otra llave, E_TIPO; sin llave, E_LLAVE.
    let (x, y) = canal_crear();
    if dashos::memoria_aviso(gl, x) != -6 || dashos::memoria_aviso(y, x) != -6 || dashos::memoria_aviso(999, x) != -2 {
        return (40, 0);
    }
    for k in [x, y, gl, sin_escribir as u32, gs, copia as u32] {
        dashos::cerrar(k);
    }
    (0, trozos * 16)
}

/// Trasfondo: el tiempo de CPU por grupo (llamada 52) y los contadores de núcleos (llamada 29). 0 y los µs del hijo
/// si va bien; si no, el número del paso que falló.
fn tiempo_de_cpu(tamano: usize) -> (u64, i64) {
    use dashos::{cpu_tiempo, medir};
    // 21: lo propio se lee sin llave y no baja.
    let a = cpu_tiempo(0);
    esperar_ms(20);
    let b = cpu_tiempo(0);
    if a < 0 || b < a + 5_000 {
        return (21, 0);
    }
    // 22: un hijo que gira 300 ms en un grupo nuevo: su grupo lo cuenta, y el de arriba (el propio) también.
    let g = grupo_crear(2 * MB);
    if g <= 0 {
        return (22, 0);
    }
    let g = g as u32;
    let p = hijo_en(g, tamano, &[2]);
    if p <= 0 {
        return (22, 0);
    }
    let antes = cpu_tiempo(0);
    dashos::dormir_us(300_000);
    let del_hijo = cpu_tiempo(g);
    if del_hijo < 50_000 || cpu_tiempo(0) < antes + del_hijo / 2 {
        println!("  [grupos] CPU del hijo {} µs; la propia, de {} a {}", del_hijo, antes, cpu_tiempo(0));
        return (23, del_hijo);
    }
    // 24: muerto el hijo, lo gastado se queda (no baja) y ya no sube.
    grupo_matar(g);
    proceso_esperar(p as u32);
    let muerto = cpu_tiempo(g);
    dashos::dormir_us(50_000);
    if muerto < del_hijo || cpu_tiempo(g) != muerto {
        return (24, del_hijo);
    }
    // 25: sin derecho de leer, otra llave o una que no existe: no.
    let sin_leer = dashos::llave_duplicar(g, dashos::ESCRIBIR, 0);
    if sin_leer <= 0 || cpu_tiempo(sin_leer as u32) != -3 || cpu_tiempo(2) != -6 || cpu_tiempo(999) != -2 {
        return (25, del_hijo);
    }
    // 26: los núcleos en marcha (al menos el 0) y uno que no existe.
    let nucleos = medir(14, 0);
    if nucleos <= 0 || nucleos & 1 == 0 || medir(3, 64) >= 0 {
        return (26, del_hijo);
    }
    // 27: los µs ociosos de un núcleo no bajan nunca, aunque esté dormido sin tic (se suma su rato de ahora).
    for n in (0..4).filter(|n| nucleos & (1 << n) != 0) {
        let x = medir(3, n as u64);
        dashos::dormir_us(20_000);
        let y = medir(3, n as u64);
        if x < 0 || y < x {
            return (27, del_hijo);
        }
    }
    (0, del_hijo)
}

/// Si lo usado por el grupo `g` vuelve a 0 en 200 ms: lo de un proceso que acabó se devuelve cuando su núcleo recoge
/// sus restos, no en el acto.
fn vuelve_a_cero(g: u32) -> bool {
    for _ in 0..200 {
        if grupo_uso(g).1 == 0 {
            return true;
        }
        dashos::dormir_us(1000);
    }
    grupo_uso(g).1 == 0
}
