//! S32: el disco del sistema en vivo (arrancado de un pendrive). El cargador deja el sistema (un DashFS entero) en la
//! memoria, y el kernel lo da como objeto de memoria ("vivo"); este servicio lo sirve por el canal "disco" con el mismo
//! protocolo que el NVMe (`docs/referencia/servicios.md`), sin anillo (el servicio de archivos va entonces por el
//! canal, que ya sabe). Lo que se escribe queda en la memoria y se pierde al apagar: los discos del PC no se tocan.
//!
//! Clientes, por el nombre que les da `inicio` (como en el NVMe): `archivos` lee y escribe el disco entero (es todo
//! la partición de DashOS); `comprobador`, solo leer. Nadie más.
//!
//! Llaves: 1 el sistema ("vivo"), 2 el canal "disco".

#![no_std]
#![no_main]

use dashos::println;

dashos::programa!(principal);

const SISTEMA: u32 = 1;
const CANAL: u32 = 2;
const VA_SISTEMA: usize = 0x40_0000_0000;
const VA_VENTANAS: usize = 0x48_0000_0000;
const VENTANA: usize = 128 * 1024;
const SECTOR: u64 = 512;
const MAX_CLIENTES: usize = 4;

const ABRIR: u32 = 0;
const LEER: u32 = 1;
const ESCRIBIR: u32 = 2;
const ANILLO: u32 = 3;
const VACIAR: u32 = 5;
const DESCARTAR: u32 = 6;
const LEER_SIN_ESPERA: u32 = 7;
const MAL_FORMADO: i32 = -1;
const FUERA_DEL_DISCO: i32 = -2;
const FUERA_DE_LA_VENTANA: i32 = -3;
const SIN_SITIO: i32 = -5;
const SIN_PERMISO: i32 = -6;

#[derive(Clone, Copy)]
struct Cliente {
    marca: u64,
    ventana: u32,
    va: usize,
    solo_leer: bool,
}

fn principal() -> u64 {
    let tam = dashos::memoria_tamano(SISTEMA);
    if tam < 4096 || dashos::memoria_proyectar(SISTEMA, VA_SISTEMA, true) != 0 {
        println!("  [disco-vivo] el sistema en memoria no se proyecta");
        return 1;
    }
    let tam = tam as u64;
    let sectores = tam / SECTOR;
    let firma = unsafe { core::slice::from_raw_parts(VA_SISTEMA as *const u8, 8) };
    println!(
        "  [disco-vivo] el sistema en vivo, {} KB en memoria ({}): lo que se cambie se pierde al apagar",
        tam / 1024,
        if firma == b"DashFS\0\0" || firma == b"RedoxFS\0" { "DashFS" } else { "sin firma de DashFS" }
    );
    let mut clientes: [Option<Cliente>; MAX_CLIENTES] = [None; MAX_CLIENTES];
    let mut nombres: [(u64, [u8; 16]); 8] = [(0, [0; 16]); 8];
    let mut buf = [0u8; 256];
    let mut ll = [0u32; 8];
    loop {
        let (r, k, marca) = dashos::canal_leer_marca(CANAL, &mut buf, &mut ll);
        if r == -4 {
            return 0;
        }
        if r < 0 {
            dashos::dormir_us(10_000);
            continue;
        }
        for &l in ll.iter().take(k).skip(1) {
            dashos::cerrar(l);
        }
        if k == 0 {
            // El directorio de `inicio`: marca y nombre de cada cliente.
            if r == 24 && marca == 0 {
                let m = u64::from_le_bytes(buf[..8].try_into().unwrap());
                if let Some(e) = nombres.iter_mut().find(|e| e.0 == 0 || e.0 == m) {
                    e.0 = m;
                    e.1.copy_from_slice(&buf[8..24]);
                }
            }
            continue;
        }
        let respuesta = ll[0];
        let u32_en = |i: usize| u32::from_le_bytes(buf[i..i + 4].try_into().unwrap());
        let (orden, cuantos, primero, off) = (u32_en(0), u32_en(4) as u64, u32_en(8) as u64 | (u32_en(12) as u64) << 32, u32_en(16) as usize);
        let quien = nombres.iter().find(|e| e.0 == marca && marca != 0).map(|e| {
            let n = &e.1[..e.1.iter().position(|&c| c == 0).unwrap_or(16)];
            match n {
                b"archivos" => 1,
                b"comprobador" => 2,
                _ => 0,
            }
        });
        let mut dar = None;
        let estado = if r != 24 || marca == 0 {
            MAL_FORMADO
        } else if orden == ABRIR {
            match quien {
                Some(1) | Some(2) => {
                    let hueco = clientes.iter().position(|c| c.is_some_and(|c| c.marca == marca)).or_else(|| clientes.iter().position(|c| c.is_none()));
                    match hueco {
                        None => SIN_SITIO,
                        Some(h) => {
                            if clientes[h].is_none() {
                                let m = dashos::memoria_crear(VENTANA);
                                let va = VA_VENTANAS + h * VENTANA;
                                if m > 0 && dashos::memoria_proyectar(m, va, true) == 0 {
                                    clientes[h] = Some(Cliente { marca, ventana: m, va, solo_leer: quien == Some(2) });
                                }
                            }
                            match clientes[h] {
                                Some(c) => {
                                    let copia = dashos::llave_duplicar(c.ventana, dashos::LEER | dashos::ESCRIBIR | dashos::TRANSFERIR, 0);
                                    dar = (copia > 0).then_some(copia as u32);
                                    if dar.is_some() { 0 } else { SIN_SITIO }
                                }
                                None => SIN_SITIO,
                            }
                        }
                    }
                }
                _ => SIN_PERMISO,
            }
        } else if orden == ANILLO {
            // Sin anillo: el servicio de archivos sigue por el canal.
            SIN_SITIO
        } else if matches!(orden, LEER | ESCRIBIR | LEER_SIN_ESPERA | VACIAR | DESCARTAR) {
            match clientes.iter().flatten().find(|c| c.marca == marca).copied() {
                None => MAL_FORMADO,
                Some(c) => {
                    let bytes = cuantos * SECTOR;
                    if orden == VACIAR {
                        0
                    } else if primero.checked_add(cuantos).is_none_or(|f| f > sectores) {
                        FUERA_DEL_DISCO
                    } else if orden == DESCARTAR {
                        if c.solo_leer { SIN_PERMISO } else { 0 }
                    } else if off.checked_add(bytes as usize).is_none_or(|f| f > VENTANA) {
                        FUERA_DE_LA_VENTANA
                    } else if orden == ESCRIBIR && c.solo_leer {
                        SIN_PERMISO
                    } else {
                        // Las dos zonas están proyectadas aquí: el sistema entero y la ventana de este cliente.
                        let disco = (VA_SISTEMA + (primero * SECTOR) as usize) as *mut u8;
                        let ventana = (c.va + off) as *mut u8;
                        unsafe {
                            if orden == ESCRIBIR {
                                core::ptr::copy_nonoverlapping(ventana, disco, bytes as usize);
                            } else {
                                core::ptr::copy_nonoverlapping(disco, ventana, bytes as usize);
                            }
                        }
                        0
                    }
                }
            }
        } else {
            MAL_FORMADO
        };
        let mut rsp = [0u8; 24];
        rsp[..4].copy_from_slice(&estado.to_le_bytes());
        let suyos = if clientes.iter().flatten().any(|c| c.marca == marca) { sectores } else { 0 };
        rsp[8..16].copy_from_slice(&suyos.to_le_bytes());
        rsp[16..20].copy_from_slice(&(SECTOR as u32).to_le_bytes());
        rsp[20..24].copy_from_slice(&(VENTANA as u32).to_le_bytes());
        let llaves: &[u32] = match &dar {
            Some(v) => core::slice::from_ref(v),
            None => &[],
        };
        if dashos::canal_escribir(respuesta, &rsp, llaves) != 0 {
            if let Some(v) = dar {
                dashos::cerrar(v);
            }
        }
        dashos::cerrar(respuesta);
    }
}
