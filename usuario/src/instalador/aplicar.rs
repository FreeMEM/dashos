//! S32: aplicar un plan. Lo mismo en DashOS (el servicio `instalador`) y en el PC (`pc.rs`, para probar contra discos de
//! verdad): lo que depende de dónde se corre (el sistema que se copia, crecer el DashFS, las variables del firmware,
//! encoger un NTFS) lo pone quien llama (`Entorno`).
//!
//! El orden, pensado para un corte de luz en cualquier punto (S32, "Deshacer"; la prueba de cortes lo recorre entero):
//! 1. Se vuelve a leer el disco y se comprueba el plan (que el disco no cambió y que no pisa nada).
//! 2. Copia de las tablas (las dos puntas del disco y, si se encoge, el principio de esa partición), en memoria; va a la
//!    ESP y al registro.
//! 3. Encoger (si toca): lo hace el `Entorno` (ntfsresize en un dominio). Hasta aquí ninguna tabla ha cambiado.
//! 4. La partición de DashOS, aún sin entrada en la tabla: el sistema copiado al hueco y crecido hasta su final. Si se
//!    corta, el hueco sigue libre para la tabla: se puede repetir.
//! 5. La ESP: el cargador en `EFI/DashOS/` (FAT, en el orden de `fat.rs`: nunca a medias para nadie). Con disco entero
//!    o sin ESP, una nueva, formateada antes de que la tabla la nombre.
//! 6. La tabla (`gpt::escribir_gpt`, en su orden). Cortado aquí, se reconoce la instalación a medias y se puede repetir.
//! 7. La entrada del menú de arranque (NVRAM). Si falla, DashOS está instalado pero no sale en el menú: se dice.

use super::disco::{Disco, guid_texto, leer_bytes};
use super::fat;
use super::gpt::{self, Nueva, tipos};
use super::nvram;
use super::plan::{self, DiscoVisto, Esp, Modo, Plan};
use super::reconocer;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Lo que pone quien aplica el plan.
pub trait Entorno {
    /// El sistema que se instala: un DashFS entero (el de `SYS:` y lo demás, como el del pendrive).
    fn sistema(&mut self) -> &[u8];
    /// El cargador de UEFI (`DASHOS.EFI`).
    fn cargador(&mut self) -> &[u8];
    /// Crece el DashFS que empieza en el sector `desde` del disco hasta el sector `hasta` (exclusivo).
    fn crecer(&mut self, d: &mut dyn Disco, desde: u64, hasta: u64) -> Result<(), String>;
    /// Encoge el NTFS de [desde, hasta) para que acabe en `nuevo_hasta`, sin mover el principio.
    fn encoger(&mut self, d: &mut dyn Disco, desde: u64, hasta: u64, nuevo_hasta: u64) -> Result<(), String>;
    /// Pone la entrada de arranque: (número de la ESP, [desde, hasta), GUID de la ESP). Devuelve el número `Boot####`.
    fn nvram(&mut self, numero: u32, desde: u64, hasta: u64, guid: &[u8; 16]) -> Result<u16, String>;
    /// Una línea del registro de lo hecho (y del avance, para la ventana).
    fn apuntar(&mut self, linea: &str);
    /// Una GUID nueva al azar (para el modo disco entero, si el plan no la trae).
    fn azar(&mut self) -> u64;
}

/// Lee lo que ve el instalador de un disco.
pub fn ver(d: &mut dyn Disco, nombre: &str, modelo: &str, de_arranque: &[u8; 16]) -> Result<DiscoVisto, String> {
    let tabla = gpt::leer(d).map_err(|e| format!("no se lee el disco: {:?}", e))?;
    let contenidos = tabla.particiones.iter().map(|p| reconocer::reconocer(d, p.desde, p.hasta)).collect();
    let es_arranque = *de_arranque != [0; 16] && tabla.particiones.iter().any(|p| p.guid == *de_arranque);
    Ok(DiscoVisto { nombre: String::from(nombre), modelo: String::from(modelo), sectores: d.sectores(), sector: d.tam_sector(), tabla, contenidos, de_arranque: es_arranque })
}

/// La copia de las tablas: las dos puntas del disco (34 y 33 sectores de 512) y, si se encoge, el primer MiB de esa
/// partición. Formato: "DashOS-tablas 1\n", y por cada trozo una línea "trozo <byte> <largo>\n" seguida de sus bytes.
pub fn copia_tablas(d: &mut dyn Disco, extra: Option<u64>) -> Result<Vec<u8>, String> {
    let ts = d.tam_sector() as u64;
    let total = d.sectores() * ts;
    let mut trozos = alloc::vec![(0u64, 34 * 512u64), (total - 33 * 512, 33 * 512)];
    if let Some(s) = extra {
        trozos.push((s * ts, 1 << 20));
    }
    let mut v = Vec::from(&b"DashOS-tablas 1\n"[..]);
    for (desde, largo) in trozos {
        let b = leer_bytes(d, desde, largo as usize).ok_or("no se leen las tablas para la copia")?;
        v.extend_from_slice(format!("trozo {} {}\n", desde, largo).as_bytes());
        v.extend_from_slice(&b);
    }
    Ok(v)
}

fn guid_de(e: &mut dyn Entorno) -> [u8; 16] {
    let (a, b) = (e.azar(), e.azar());
    let mut g = [0u8; 16];
    g[..8].copy_from_slice(&a.to_le_bytes());
    g[8..].copy_from_slice(&b.to_le_bytes());
    g[7] = (g[7] & 0x0F) | 0x40;
    g[8] = (g[8] & 0x3F) | 0x80;
    g
}

/// Aplica el plan al disco. `de_arranque` es el GUID de la partición desde la que arrancó DashOS (el pendrive).
pub fn aplicar(d: &mut dyn Disco, p: &Plan, nombre: &str, modelo: &str, de_arranque: &[u8; 16], e: &mut dyn Entorno) -> Result<(), String> {
    let ts = d.tam_sector() as u64;
    // 1.
    let v = ver(d, nombre, modelo, de_arranque)?;
    plan::comprobar(p, &v)?;
    e.apuntar(&format!("plan comprobado: disco {}, modo {:?}", p.disco, p.modo));
    // 2.
    let extra = p.encoger.and_then(|(n, _)| v.tabla.particiones.iter().find(|q| q.numero == n)).map(|q| q.desde);
    let copia = copia_tablas(d, extra)?;
    e.apuntar(&format!("copia de las tablas hecha ({} bytes)", copia.len()));
    // 3.
    if let Some((n, nuevo)) = p.encoger {
        let q = v.tabla.particiones.iter().find(|q| q.numero == n).ok_or("la partición que se encoge no está")?.clone();
        e.apuntar(&format!("encoger la partición {}: de {} a {} sectores", n, q.hasta - q.desde, nuevo - q.desde));
        e.encoger(d, q.desde, q.hasta, nuevo)?;
        e.apuntar("encogida");
    }
    // La tabla de partida: la del disco, o una nueva (disco entero).
    let tabla = if p.modo == Modo::DiscoEntero { gpt::tabla_vacia(p.sectores, ts as usize, p.disco_guid.unwrap()) } else { v.tabla.clone() };
    // 4.
    let (a, b, guid_dashos) = p.dashos;
    let sistema = e.sistema().to_vec();
    if sistema.len() as u64 > (b - a) * ts {
        return Err(String::from("el sistema no cabe en la partición"));
    }
    if &sistema[..8] != b"DashFS\0\0" && &sistema[..8] != b"RedoxFS\0" {
        return Err(String::from("el sistema que se instala no es un DashFS"));
    }
    e.apuntar(&format!("copiando el sistema ({} KB) a los sectores {}..{}", sistema.len() / 1024, a, b));
    let trozo = (1usize << 20).next_multiple_of(ts as usize);
    for (k, t) in sistema.chunks(trozo).enumerate() {
        let mut t = t.to_vec();
        t.resize((t.len() as u64).div_ceil(ts) as usize * ts as usize, 0);
        d.escribir(a + (k * trozo) as u64 / ts, &t).map_err(|_| "no se pudo escribir el sistema")?;
    }
    d.vaciar().map_err(|_| "no se pudo escribir el sistema")?;
    e.crecer(d, a, b)?;
    e.apuntar("sistema copiado y crecido hasta el final de su partición");
    // 5.
    let (esp_num, esp_desde, esp_hasta, esp_guid, nueva_esp) = match p.esp {
        Esp::Existente(n) => {
            let q = tabla.particiones.iter().find(|q| q.numero == n).ok_or("la ESP no está")?;
            (n, q.desde, q.hasta, q.guid, None)
        }
        Esp::Nueva(x, y, g) => {
            e.apuntar("formateando la partición de arranque EFI nueva (FAT32)");
            fat::formatear_fat32(d, x, y, "DASHOS-EFI", e.azar() as u32).map_err(String::from)?;
            // Su número: el de la primera entrada libre de la lista (la que va a tomar en el paso 6).
            let te = tabla.cabecera.unwrap().tam_e as usize;
            let libre = (0..tabla.cabecera.unwrap().n as usize).find(|k| tabla.lista[k * te..k * te + 16].iter().all(|&c| c == 0)).ok_or("no hay entrada libre en la tabla")?;
            (libre as u32 + 1, x, y, g, Some(Nueva { tipo: tipos::ESP, guid: g, desde: x, hasta: y, nombre: "EFI system partition" }))
        }
    };
    let mut f = fat::Fat::abrir(d, esp_desde, esp_hasta).map_err(|m| format!("la partición de arranque EFI: {}", m))?;
    let cargador = e.cargador().to_vec();
    let falta = (cargador.len() + copia.len() + 64 * 1024) as u64;
    if (f.libres() as u64) * f.bytes_cluster() < falta {
        return Err(format!("no hay sitio en la partición de arranque EFI: hacen falta {} KB", falta / 1024));
    }
    let efi = f.cajon(d, 0, "EFI").map_err(String::from)?;
    let suyo = f.cajon(d, efi, "DashOS").map_err(String::from)?;
    f.poner_fichero(d, suyo, "TABLAS.BIN", &copia).map_err(String::from)?;
    f.poner_fichero(d, suyo, "DASHOS.EFI", &cargador).map_err(String::from)?;
    let mut g = String::new();
    guid_texto(&guid_dashos, &mut g);
    let leeme = format!(
        "DashOS\r\n\r\nEl cargador de DashOS (DASHOS.EFI) y la copia de las tablas de particiones de antes de instalarlo\r\n(TABLAS.BIN). DashOS esta en la particion {}.\r\nNada de esta carpeta es de otro sistema; lo de las demas carpetas no se ha tocado.\r\n",
        g
    );
    f.poner_fichero(d, suyo, "LEEME.TXT", leeme.as_bytes()).map_err(String::from)?;
    e.apuntar("cargador puesto en EFI\\DashOS de la partición de arranque EFI");
    // 6.
    if p.modo != Modo::Reinstalar {
        let mut nuevas = Vec::new();
        if let Some(n) = nueva_esp {
            nuevas.push(n);
        }
        nuevas.push(Nueva { tipo: tipos::DASHOS, guid: guid_dashos, desde: a, hasta: b, nombre: "DashOS" });
        // Una instalación cortada a medias deja una entrada de DashOS en una copia: se quita y se pone la buena.
        let quitar = tabla.copias == gpt::Copias::SoloDashOS;
        let lista = gpt::lista_nueva(&tabla, &nuevas, quitar).map_err(|f| format!("la tabla nueva: {:?}", f))?;
        if p.modo == Modo::DiscoEntero {
            // El MBR protector, primero: sin él nadie mira la GPT; con él y sin GPT buena, nadie usa el disco.
            d.escribir(0, &gpt::mbr_protector(p.sectores, ts as usize)).map_err(|_| "no se pudo escribir el MBR")?;
        }
        gpt::escribir_gpt(d, &tabla, &lista).map_err(|f| format!("escribiendo la tabla: {:?}", f))?;
        e.apuntar("tabla de particiones escrita (las dos copias)");
        let otra = gpt::leer(d).map_err(|_| "no se relee la tabla")?;
        if otra.copias != gpt::Copias::Iguales || !otra.particiones.iter().any(|q| q.guid == guid_dashos && q.tipo == tipos::DASHOS) {
            return Err(String::from("la tabla escrita no se lee bien"));
        }
    }
    // 7.
    if p.nvram {
        match e.nvram(esp_num, esp_desde, esp_hasta, &esp_guid) {
            Ok(n) => e.apuntar(&format!("entrada {} \"DashOS\" en el menú de arranque del firmware ({}), y es la del próximo arranque", nvram::nombre_boot(n), nvram::CAMINO)),
            Err(m) => {
                e.apuntar(&format!("AVISO: DashOS está instalado, pero no se pudo añadir al menú de arranque: {}", m));
                return Err(format!("instalado, pero sin entrada en el menú de arranque: {}", m));
            }
        }
    }
    e.apuntar("instalación terminada");
    Ok(())
}

/// Prepara lo de NVRAM para `Entorno::nvram`: con las entradas que hay y el `BootOrder`, lo que hay que escribir. Ver
/// `nvram::decidir`.
pub fn entrada_nvram(numero: u32, desde: u64, hasta: u64, guid: &[u8; 16]) -> Vec<u8> {
    nvram::opcion(nvram::DESCRIPCION, numero, desde, hasta, guid, nvram::CAMINO)
}
