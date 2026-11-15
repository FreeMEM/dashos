//! El intérprete de guiones de AmigaDOS, como una máquina de estados: `paso` hace una línea y dice qué toca. Lo que
//! no es de control (`Dir`, `Copy`, `Set`...) se lo devuelve a quien lo conduce (`Paso::Ejecutar`), que lo lanza y
//! avisa con `acabada` cuando termina. Así el Shell no se queda parado dentro de un guion: entre paso y paso atiende
//! su ventana, y un guion con un bucle sin fin se corta con Ctrl-C (`romper`).
//!
//! De control, aquí: `If`/`Else`/`EndIf`, `Skip`/`Lab`/`EndSkip`, `FailAt`, `Quit`, `Execute`, `EndShell` (y `EndCLI`).
//! Como en AmigaDOS, sin pila de `If`: un `If` falso busca hacia delante su `Else` o su `EndIf`; un `Else` que se
//! alcanza ejecutando salta a su `EndIf`. Por eso `Skip` puede saltar a cualquier sitio sin dejar nada a medias.
//!
//! Los guiones con `.KEY plantilla` reciben argumentos: `<nombre>` se cambia por su valor, `<nombre$defecto>` por el
//! valor o por el defecto; `.BRA` y `.KET` cambian los corchetes y `.DEF nombre valor` da un defecto.

use crate::linea::{self, Orden};
use crate::{plantilla, MAX_LINEA, RC_AVISO, RC_ERROR, RC_FALLO};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Guiones unos dentro de otros, como mucho (un guion que se ejecuta a sí mismo no se come la memoria).
pub const MAX_NIVELES: usize = 16;
/// Líneas de un guion, como mucho.
pub const MAX_LINEAS: usize = 20_000;

/// Lo que el intérprete necesita de fuera.
pub trait Mundo {
    /// Un mensaje del intérprete para el usuario (una línea, sin el salto).
    fn decir(&mut self, texto: &str);
    /// El texto del guion `ruta` (como la escribió el usuario), o el estado del servicio de archivos.
    fn leer_guion(&mut self, ruta: &str) -> Result<String, i32>;
    /// Si existe `ruta` (para `If EXISTS`).
    fn existe(&mut self, ruta: &str) -> bool;
    /// Una variable de entorno (`ENV:`), si está.
    fn env(&mut self, nombre: &str) -> Option<String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Paso {
    /// Se hizo una línea (o nada); se puede seguir.
    Nada,
    /// Hay que ejecutar esto (una orden, o varias en tubería) y avisar con `acabada`, o con `guion` si era un guion.
    Ejecutar(Vec<Orden>),
    /// No queda nada por hacer: toca pedir una línea.
    Pedir,
    /// `EndShell`: el Shell acaba, con este código.
    Fin(i32),
}

struct Marco {
    nombre: String,
    lineas: Vec<String>,
    pc: usize,
    /// El `FailAt` de antes de entrar (se repone al salir de un guion).
    falla_en_antes: i32,
    /// Si es una línea tecleada, no un guion.
    tecleada: bool,
    /// Los argumentos con que se llamó y, tras `.KEY`, sus valores por nombre.
    args: String,
    parametros: Vec<(String, String)>,
    bra: char,
    ket: char,
}

pub struct Interprete {
    marcos: Vec<Marco>,
    /// Las variables locales (`Set`).
    pub locales: Vec<(String, String)>,
    /// Los alias (`Alias`).
    pub alias: Vec<(String, String)>,
    /// El código de la última orden y su segundo resultado (`$RC`, `$Result2`).
    pub rc: i32,
    pub resultado2: i32,
    /// `FailAt`: con un código así o mayor, un guion se para.
    pub falla_en: i32,
    esperando: bool,
    /// El nombre de lo último que se mandó ejecutar (para decir qué falló).
    ultima: String,
}

impl Default for Interprete {
    fn default() -> Self {
        Self::nuevo()
    }
}

/// La primera palabra de una línea (sin espacios delante, sin comentario) y el resto.
fn primera_palabra(linea: &str) -> (&str, &str) {
    let l = linea::sin_comentario(linea).trim();
    match l.find([' ', '\t']) {
        Some(i) => (&l[..i], l[i..].trim_start()),
        None => (l, ""),
    }
}

fn es(palabra: &str, orden: &str) -> bool {
    palabra.eq_ignore_ascii_case(orden)
}

impl Interprete {
    pub fn nuevo() -> Self {
        Interprete { marcos: Vec::new(), locales: Vec::new(), alias: Vec::new(), rc: 0, resultado2: 0, falla_en: RC_ERROR, esperando: false, ultima: String::new() }
    }

    /// Si hay algo a medias (un guion, o una línea tecleada sin acabar).
    pub fn ocupado(&self) -> bool {
        !self.marcos.is_empty() || self.esperando
    }

    /// Si se está dentro de un guion (no solo de una línea tecleada).
    pub fn en_guion(&self) -> bool {
        self.marcos.iter().any(|m| !m.tecleada)
    }

    /// Una línea tecleada: se hará con los `paso` siguientes.
    pub fn linea(&mut self, texto: &str) {
        self.marcos.push(Marco {
            nombre: String::new(),
            lineas: alloc::vec![String::from(texto)],
            pc: 0,
            falla_en_antes: self.falla_en,
            tecleada: true,
            args: String::new(),
            parametros: Vec::new(),
            bra: '<',
            ket: '>',
        });
    }

    /// Entra en un guion: `texto`, con los argumentos `args`. Falso (y código 20) si ya hay demasiados niveles.
    pub fn guion(&mut self, m: &mut dyn Mundo, nombre: &str, texto: &str, args: &str) -> bool {
        self.esperando = false;
        if self.marcos.iter().filter(|x| !x.tecleada).count() >= MAX_NIVELES {
            m.decir(&format!("{}: demasiados guiones unos dentro de otros", nombre));
            self.fallo(m, RC_FALLO, 0);
            return false;
        }
        let lineas: Vec<String> = texto.lines().take(MAX_LINEAS + 1).map(String::from).collect();
        if lineas.len() > MAX_LINEAS {
            m.decir(&format!("{}: el guion tiene más de {} líneas", nombre, MAX_LINEAS));
            self.fallo(m, RC_FALLO, 0);
            return false;
        }
        self.marcos.push(Marco {
            nombre: String::from(nombre),
            lineas,
            pc: 0,
            falla_en_antes: self.falla_en,
            tecleada: false,
            args: String::from(args),
            parametros: Vec::new(),
            bra: '<',
            ket: '>',
        });
        true
    }

    /// Lo que se mandó ejecutar acabó con `rc` (y su segundo resultado). Con un código de `FailAt` o más, se dice y se
    /// para todo lo que hubiera a medias.
    pub fn acabada(&mut self, m: &mut dyn Mundo, rc: i32, resultado2: i32) {
        self.esperando = false;
        self.fallo(m, rc, resultado2);
    }

    fn fallo(&mut self, m: &mut dyn Mundo, rc: i32, resultado2: i32) {
        self.rc = rc;
        self.resultado2 = resultado2;
        if rc >= self.falla_en {
            if !self.ultima.is_empty() {
                m.decir(&format!("{} falló con el código {}", self.ultima, rc));
            }
            self.vaciar();
        }
    }

    /// Fuera todo lo que hubiera a medias; el `FailAt`, el de antes de los guiones.
    fn vaciar(&mut self) {
        if let Some(primero) = self.marcos.first() {
            self.falla_en = primero.falla_en_antes;
        }
        self.marcos.clear();
        self.esperando = false;
    }

    /// Ctrl-C: se para lo que hubiera a medias. Dice si había algo.
    pub fn romper(&mut self, m: &mut dyn Mundo) -> bool {
        let habia = self.ocupado();
        if habia {
            self.vaciar();
            self.rc = RC_FALLO;
            m.decir("***Break");
        }
        habia
    }

    /// El valor de una variable: local, `RC` y `Result2`, o de entorno.
    pub fn variable(&self, m: &mut dyn Mundo, nombre: &str) -> Option<String> {
        if let Some((_, v)) = self.locales.iter().find(|(n, _)| n.eq_ignore_ascii_case(nombre)) {
            return Some(v.clone());
        }
        if nombre.eq_ignore_ascii_case("RC") {
            return Some(self.rc.to_string());
        }
        if nombre.eq_ignore_ascii_case("Result2") {
            return Some(self.resultado2.to_string());
        }
        m.env(nombre)
    }

    /// Pone (o cambia) una variable local.
    pub fn poner_local(&mut self, nombre: &str, valor: &str) {
        match self.locales.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(nombre)) {
            Some(v) => v.1 = String::from(valor),
            None => self.locales.push((String::from(nombre), String::from(valor))),
        }
    }

    /// Hace una línea de lo que hay a medias.
    pub fn paso(&mut self, m: &mut dyn Mundo) -> Paso {
        if self.esperando {
            return Paso::Nada;
        }
        let Some(marco) = self.marcos.last_mut() else { return Paso::Pedir };
        if marco.pc >= marco.lineas.len() {
            let acabado = self.marcos.pop().unwrap();
            if !acabado.tecleada {
                self.falla_en = acabado.falla_en_antes;
            }
            return Paso::Nada;
        }
        let cruda = core::mem::take(&mut marco.lineas[marco.pc]);
        // La línea vuelve a su sitio: `Skip BACK` puede pasar otra vez por ella.
        marco.lineas[marco.pc] = cruda.clone();
        marco.pc += 1;
        let tecleada = marco.tecleada;
        if cruda.len() > MAX_LINEA {
            m.decir("línea demasiado larga");
            self.ultima.clear();
            self.fallo(m, RC_FALLO, 0);
            return Paso::Nada;
        }
        // Las líneas con punto de un guion: `.KEY` y compañía.
        if !tecleada && cruda.trim_start().starts_with('.') {
            self.directiva(m, cruda.trim_start());
            return Paso::Nada;
        }
        let con_parametros = self.poner_parametros(&cruda);
        let sin = linea::sin_comentario(&con_parametros);
        let Some(puesta) = linea::sustituir(sin, MAX_LINEA, |n| self.variable(m, n)) else {
            m.decir("línea demasiado larga");
            self.ultima.clear();
            self.fallo(m, RC_FALLO, 0);
            return Paso::Nada;
        };
        let mut ordenes = match linea::partir(&puesta) {
            Ok(o) => o,
            Err(e) => {
                m.decir(&format!("{}", e));
                self.ultima.clear();
                self.fallo(m, RC_FALLO, 0);
                return Paso::Nada;
            }
        };
        if ordenes.is_empty() {
            return Paso::Nada;
        }
        // Un alias cambia el nombre de la primera orden por su texto (una vez: un alias no llama a otro).
        if let Some((_, valor)) = self.alias.iter().find(|(n, _)| n.eq_ignore_ascii_case(&ordenes[0].nombre)) {
            let (nombre, resto) = primera_palabra(valor);
            let mut args = String::from(resto);
            if !ordenes[0].args.is_empty() {
                if !args.is_empty() {
                    args.push(' ');
                }
                args.push_str(&ordenes[0].args);
            }
            ordenes[0].nombre = String::from(nombre);
            ordenes[0].args = args;
        }
        if ordenes.len() == 1 && let Some(p) = self.control(m, &ordenes[0], tecleada) {
            return p;
        }
        self.ultima = ordenes.last().map(|o| o.nombre.clone()).unwrap_or_default();
        self.esperando = true;
        Paso::Ejecutar(ordenes)
    }

    /// Cambia `<nombre>` y `<nombre$defecto>` por los argumentos del guion (tras `.KEY`).
    fn poner_parametros(&self, cruda: &str) -> String {
        let Some(marco) = self.marcos.last().filter(|x| !x.parametros.is_empty()) else { return String::from(cruda) };
        let mut sal = String::new();
        let mut resto = cruda;
        while let Some(i) = resto.find(marco.bra) {
            sal.push_str(&resto[..i]);
            let tras = &resto[i + marco.bra.len_utf8()..];
            let Some(f) = tras.find(marco.ket) else {
                sal.push(marco.bra);
                resto = tras;
                continue;
            };
            let dentro = &tras[..f];
            let (nombre, defecto) = dentro.split_once('$').map_or((dentro, None), |(n, d)| (n, Some(d)));
            match marco.parametros.iter().find(|(n, _)| n.eq_ignore_ascii_case(nombre)) {
                Some((_, v)) => {
                    sal.push_str(if v.is_empty() { defecto.unwrap_or("") } else { v });
                    resto = &tras[f + marco.ket.len_utf8()..];
                }
                // No es un parámetro (una redirección, `<fichero>`...): se queda.
                None => {
                    sal.push(marco.bra);
                    resto = tras;
                }
            }
        }
        sal.push_str(resto);
        sal
    }

    fn directiva(&mut self, m: &mut dyn Mundo, l: &str) {
        let (palabra, resto) = primera_palabra_cruda(l);
        let palabra = palabra.to_ascii_uppercase();
        let marco = self.marcos.last_mut().unwrap();
        match palabra.as_str() {
            ".KEY" | ".K" => {
                // Los campos de la plantilla, con el valor que traiga cada uno.
                match plantilla::leer(resto, &marco.args) {
                    Ok(a) => {
                        marco.parametros = resto
                            .split(',')
                            .filter_map(|campo| {
                                let nombres = campo.split('/').next().unwrap_or("");
                                let interruptor = campo.split('/').skip(1).any(|x| x.eq_ignore_ascii_case("S"));
                                let n = nombres.split('=').next().unwrap_or("");
                                if n.is_empty() {
                                    return None;
                                }
                                let v = if interruptor {
                                    if a.si(n) { String::from(n) } else { String::new() }
                                } else if !a.varios(n).is_empty() {
                                    a.varios(n).join(" ")
                                } else {
                                    a.texto(n).map(String::from).unwrap_or_default()
                                };
                                Some((String::from(n), v))
                            })
                            .collect();
                        // Sin campos con nombre, que al menos conste que hay parámetros (para no mirar más).
                    }
                    Err(e) => {
                        let nombre = marco.nombre.clone();
                        m.decir(&format!("{}: {}", nombre, e));
                        self.ultima.clear();
                        self.fallo(m, RC_FALLO, 0);
                    }
                }
            }
            ".BRA" => marco.bra = resto.chars().next().unwrap_or('<'),
            ".KET" => marco.ket = resto.chars().next().unwrap_or('>'),
            ".DEF" => {
                let (n, v) = primera_palabra_cruda(resto);
                if let Some(p) = marco.parametros.iter_mut().find(|(x, _)| x.eq_ignore_ascii_case(n)) {
                    if p.1.is_empty() {
                        p.1 = String::from(v);
                    }
                }
            }
            // Un punto solo es un comentario; lo demás (`.DOT`, `.DOLLAR`), sin hacer, se salta.
            _ => {}
        }
    }

    /// Las órdenes de control. `None` si `o` no es una.
    fn control(&mut self, m: &mut dyn Mundo, o: &Orden, tecleada: bool) -> Option<Paso> {
        let n = o.nombre.as_str();
        let de_guion = ["if", "else", "endif", "skip", "lab", "endskip", "quit"].iter().any(|p| es(n, p));
        if de_guion && tecleada {
            m.decir(&format!("{} solo vale dentro de un guion", n));
            self.ultima.clear();
            self.fallo(m, RC_ERROR, 0);
            return Some(Paso::Nada);
        }
        if es(n, "if") {
            let cierto = match self.condicion(m, &o.args) {
                Ok(c) => c,
                Err(e) => {
                    m.decir(&format!("If: {}", e));
                    self.ultima.clear();
                    self.fallo(m, RC_FALLO, 0);
                    return Some(Paso::Nada);
                }
            };
            if !cierto && !self.saltar_bloque(true) {
                m.decir("If sin su EndIf");
                self.ultima.clear();
                self.fallo(m, RC_FALLO, 0);
            }
        } else if es(n, "else") {
            if !self.saltar_bloque(false) {
                m.decir("Else sin su EndIf");
                self.ultima.clear();
                self.fallo(m, RC_FALLO, 0);
            }
        } else if es(n, "endif") || es(n, "lab") || es(n, "endskip") {
        } else if es(n, "skip") {
            let a = match plantilla::leer("LABEL,BACK/S", &o.args) {
                Ok(a) => a,
                Err(e) => {
                    m.decir(&format!("Skip: {}", e));
                    self.ultima.clear();
                    self.fallo(m, RC_FALLO, 0);
                    return Some(Paso::Nada);
                }
            };
            let etiqueta = a.texto("LABEL").unwrap_or("");
            let marco = self.marcos.last_mut().unwrap();
            let desde = if a.si("BACK") { 0 } else { marco.pc };
            let hallada = (desde..marco.lineas.len()).find(|&i| {
                let (p, resto) = primera_palabra(&marco.lineas[i]);
                (es(p, "lab") && resto.eq_ignore_ascii_case(etiqueta)) || (es(p, "endskip") && !a.si("BACK"))
            });
            match hallada {
                Some(i) => {
                    let en_endskip = es(primera_palabra(&marco.lineas[i]).0, "endskip");
                    marco.pc = i + 1;
                    if en_endskip {
                        // Se llegó a un `EndSkip` sin encontrar la etiqueta: aviso, como en AmigaDOS.
                        self.ultima = String::from("Skip");
                        self.fallo(m, RC_AVISO, 0);
                    }
                }
                None => {
                    m.decir(&format!("Skip: no está la etiqueta \"{}\"", etiqueta));
                    self.ultima.clear();
                    self.fallo(m, RC_FALLO, 0);
                }
            }
        } else if es(n, "quit") {
            let rc = match plantilla::leer("RC/N", &o.args) {
                Ok(a) => a.numero("RC").unwrap_or(0) as i32,
                Err(_) => RC_FALLO,
            };
            if let Some(acabado) = self.marcos.pop() {
                self.falla_en = acabado.falla_en_antes;
                self.ultima = acabado.nombre;
            }
            self.fallo(m, rc, 0);
        } else if es(n, "failat") {
            match plantilla::leer("RCLIM/N", &o.args) {
                Ok(a) => match a.numero("RCLIM") {
                    Some(v) if v > 0 => self.falla_en = v.min(i32::MAX as i64) as i32,
                    Some(_) => m.decir("FailAt: el límite tiene que ser mayor que 0"),
                    None => m.decir(&format!("Límite de fallo: {}", self.falla_en)),
                },
                Err(e) => m.decir(&format!("FailAt: {}", e)),
            }
        } else if es(n, "execute") {
            let (fichero, args) = match plantilla::leer("FILE/A,ARGS/F", &o.args) {
                Ok(a) => (a.texto("FILE").unwrap_or("").to_string(), a.texto("ARGS").unwrap_or("").to_string()),
                Err(e) => {
                    m.decir(&format!("Execute: {}", e));
                    self.ultima.clear();
                    self.fallo(m, RC_FALLO, 0);
                    return Some(Paso::Nada);
                }
            };
            match m.leer_guion(&fichero) {
                Ok(texto) => {
                    self.guion(m, &fichero, &texto, &args);
                }
                Err(e) => {
                    m.decir(&format!("Execute: no se puede leer {} ({})", fichero, e));
                    self.ultima.clear();
                    self.fallo(m, RC_FALLO, e);
                }
            }
        } else if es(n, "endshell") || es(n, "endcli") {
            self.vaciar();
            return Some(Paso::Fin(self.rc));
        } else {
            return None;
        }
        Some(Paso::Nada)
    }

    /// Salta el bloque de un `If` falso (hasta detrás de su `Else` o de su `EndIf`) o, desde un `Else` alcanzado, hasta
    /// detrás de su `EndIf`. Falso si no se encuentra.
    fn saltar_bloque(&mut self, hasta_else: bool) -> bool {
        let marco = self.marcos.last_mut().unwrap();
        let mut nivel = 0;
        for i in marco.pc..marco.lineas.len() {
            let (p, _) = primera_palabra(&marco.lineas[i]);
            if es(p, "if") {
                nivel += 1;
            } else if es(p, "endif") {
                if nivel == 0 {
                    marco.pc = i + 1;
                    return true;
                }
                nivel -= 1;
            } else if es(p, "else") && nivel == 0 && hasta_else {
                marco.pc = i + 1;
                return true;
            }
        }
        marco.pc = marco.lineas.len();
        false
    }

    /// La condición de un `If`.
    fn condicion(&mut self, m: &mut dyn Mundo, args: &str) -> Result<bool, plantilla::Error> {
        let a = plantilla::leer("NOT/S,WARN/S,ERROR/S,FAIL/S,,EQ/K,GT/K,GE/K,VAL/S,EXISTS/K", args)?;
        let izquierda = a.posicion(4).unwrap_or("");
        let comparar = |otro: &str| -> core::cmp::Ordering {
            if a.si("VAL") {
                izquierda.trim().parse::<i64>().unwrap_or(0).cmp(&otro.trim().parse::<i64>().unwrap_or(0))
            } else {
                izquierda.to_lowercase().cmp(&otro.to_lowercase())
            }
        };
        let mut cierto = false;
        let mut alguna = false;
        let mut con = |c: bool| {
            cierto = if alguna { cierto && c } else { c };
            alguna = true;
        };
        if a.si("WARN") {
            con(self.rc >= RC_AVISO);
        }
        if a.si("ERROR") {
            con(self.rc >= RC_ERROR);
        }
        if a.si("FAIL") {
            con(self.rc >= RC_FALLO);
        }
        if let Some(o) = a.texto("EQ") {
            con(comparar(o).is_eq());
        }
        if let Some(o) = a.texto("GT") {
            con(comparar(o).is_gt());
        }
        if let Some(o) = a.texto("GE") {
            con(comparar(o).is_ge());
        }
        if let Some(f) = a.texto("EXISTS") {
            con(m.existe(f));
        }
        Ok(cierto != a.si("NOT"))
    }
}

/// Como `primera_palabra`, sin quitar comentarios (para las líneas con punto).
fn primera_palabra_cruda(l: &str) -> (&str, &str) {
    let l = l.trim();
    match l.find([' ', '\t']) {
        Some(i) => (&l[..i], l[i..].trim_start()),
        None => (l, ""),
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use alloc::vec;

    #[derive(Default)]
    struct Falso {
        dicho: Vec<String>,
        guiones: Vec<(String, String)>,
        existen: Vec<String>,
        /// Lo que se "ejecutó": nombre y argumentos.
        hechas: Vec<String>,
    }

    impl Mundo for Falso {
        fn decir(&mut self, texto: &str) {
            self.dicho.push(String::from(texto));
        }

        fn leer_guion(&mut self, ruta: &str) -> Result<String, i32> {
            self.guiones.iter().find(|g| g.0 == ruta).map(|g| g.1.clone()).ok_or(-2)
        }

        fn existe(&mut self, ruta: &str) -> bool {
            self.existen.iter().any(|e| e == ruta)
        }

        fn env(&mut self, nombre: &str) -> Option<String> {
            (nombre == "Idioma").then(|| String::from("español"))
        }
    }

    /// Conduce el intérprete hasta que pide una línea: las órdenes "se ejecutan" apuntándolas; `Falla n` devuelve n,
    /// `Set a b` pone una variable local. Como mucho `tope` pasos (para los bucles sin fin).
    fn conducir(i: &mut Interprete, m: &mut Falso, tope: usize) -> (Paso, usize) {
        for n in 0..tope {
            match i.paso(m) {
                Paso::Nada => {}
                Paso::Ejecutar(ordenes) => {
                    let o = &ordenes[0];
                    m.hechas.push(format!("{} {}", o.nombre, o.args).trim().to_string());
                    if o.nombre.eq_ignore_ascii_case("Set") {
                        let (n, v) = o.args.split_once(' ').unwrap_or((&o.args, ""));
                        i.poner_local(n, v);
                        i.acabada(m, 0, 0);
                    } else if o.nombre.eq_ignore_ascii_case("Falla") {
                        i.acabada(m, o.args.parse().unwrap_or(20), 0);
                    } else {
                        i.acabada(m, 0, 0);
                    }
                }
                p => return (p, n),
            }
        }
        (Paso::Nada, tope)
    }

    fn correr(guion: &str, args: &str) -> (Falso, Interprete) {
        let mut m = Falso::default();
        m.guiones.push(("g".into(), guion.into()));
        m.existen.push("SYS:C".into());
        let mut i = Interprete::nuevo();
        i.linea(&format!("Execute g {}", args));
        let (p, _) = conducir(&mut i, &mut m, 10_000);
        assert_eq!(p, Paso::Pedir);
        (m, i)
    }

    #[test]
    fn if_else_endif() {
        let (m, _) = correr("If EXISTS SYS:C\n  Echo uno\n  If EXISTS nada\n    Echo no\n  Else\n    Echo dos\n  EndIf\nElse\n  Echo tampoco\nEndIf\nEcho fin", "");
        assert_eq!(m.hechas, ["Echo uno", "Echo dos", "Echo fin"]);
        let (m, _) = correr("If NOT EXISTS SYS:C\n  Echo no\n  If EXISTS SYS:C\n Echo no\n Else\n Echo no\n EndIf\nElse\n  Echo sí\nEndIf", "");
        assert_eq!(m.hechas, ["Echo sí"]);
        let (m, _) = correr("If a EQ A\nEcho iguales\nEndIf\nIf 10 GT 9\nEcho texto\nEndIf\nIf 10 GT 9 VAL\nEcho número\nEndIf\nIf 3 GE 3 VAL\nEcho ge\nEndIf", "");
        assert_eq!(m.hechas, ["Echo iguales", "Echo número", "Echo ge"]);
    }

    #[test]
    fn codigos_y_failat() {
        // Un aviso (5) no para el guion; un error (10), sí, y se dice cuál.
        let (m, i) = correr("Falla 5\nIf WARN\nEcho aviso $RC\nEndIf\nFalla 10\nEcho no llega", "");
        assert_eq!(m.hechas, ["Falla 5", "Echo aviso 5", "Falla 10"]);
        assert_eq!(m.dicho, ["Falla falló con el código 10"]);
        assert_eq!(i.rc, 10);
        // Con FailAt 21 sigue, y los códigos se pueden mirar.
        let (m, i) = correr("FailAt 21\nFalla 20\nIf FAIL\nEcho falló\nEndIf\nIf ERROR\nEcho no\nEndIf\nFailAt", "");
        assert_eq!(m.hechas, ["Falla 20", "Echo falló"]);
        assert_eq!(m.dicho, ["Límite de fallo: 21"]);
        // Al salir del guion, el límite vuelve al de antes.
        assert_eq!(i.falla_en, 10);
    }

    #[test]
    fn skip_y_lab() {
        let (m, _) = correr("Skip fin\nEcho no\nLab otra\nEcho no\nLab FIN\nEcho sí", "");
        assert_eq!(m.hechas, ["Echo sí"]);
        // Hacia atrás, con una variable que corta el bucle.
        let (m, _) = correr("Set n uno\nLab otra\nIf $n EQ dos\n  Skip fin\nEndIf\nEcho vuelta $n\nSet n dos\nSkip otra BACK\nLab fin\nEcho fuera", "");
        assert_eq!(m.hechas, ["Set n uno", "Echo vuelta uno", "Set n dos", "Echo fuera"]);
        let (m, i) = correr("Skip nada\nEcho no", "");
        assert_eq!((m.hechas.len(), i.rc), (0, 20));
        assert_eq!(m.dicho, ["Skip: no está la etiqueta \"nada\""]);
        let (m, i) = correr("FailAt 21\nSkip nada\nEcho no\nEndSkip\nEcho tras el EndSkip $RC", "");
        assert_eq!((m.hechas.as_slice(), i.rc), (&["Echo tras el EndSkip 5".to_string()][..], 0));
    }

    #[test]
    fn bucle_sin_fin_y_ctrl_c() {
        let mut m = Falso::default();
        m.guiones.push(("bucle".into(), "Lab otra\nEcho vuelta\nSkip otra BACK\nEcho no llega".into()));
        let mut i = Interprete::nuevo();
        i.linea("Execute bucle");
        let (p, pasos) = conducir(&mut i, &mut m, 3000);
        assert_eq!((p, pasos), (Paso::Nada, 3000));
        assert!(i.ocupado() && i.en_guion());
        assert!(m.hechas.len() > 900);
        // Ctrl-C: se acaba, y el Shell vuelve a pedir una línea.
        assert!(i.romper(&mut m));
        assert_eq!(m.dicho.last().map(String::as_str), Some("***Break"));
        assert_eq!(i.paso(&mut m), Paso::Pedir);
        assert!(!i.romper(&mut m));
        // Y roto mientras espera a una orden: lo que diga después esa orden no cuenta para nada.
        i.linea("Execute bucle");
        loop {
            if let Paso::Ejecutar(_) = i.paso(&mut m) {
                break;
            }
        }
        assert!(i.romper(&mut m));
        assert_eq!(i.paso(&mut m), Paso::Pedir);
    }

    #[test]
    fn un_guion_que_se_llama_a_si_mismo() {
        let mut m = Falso::default();
        m.guiones.push(("yo".into(), "Echo dentro\nExecute yo\nEcho no llega".into()));
        let mut i = Interprete::nuevo();
        i.linea("Execute yo");
        let (p, _) = conducir(&mut i, &mut m, 10_000);
        assert_eq!(p, Paso::Pedir);
        assert_eq!(m.hechas.len(), MAX_NIVELES);
        assert!(m.dicho.iter().any(|d| d.contains("demasiados guiones")));
        assert!(!i.ocupado());
    }

    #[test]
    fn parametros() {
        let (m, _) = correr(".KEY nombre/A,donde,TODO/S\n.DEF donde RAM:\nEcho <nombre> a <donde> <TODO> \"<otro$x>\" y <nombre$nadie>", "\"Ana Mar\" TODO");
        assert_eq!(m.hechas, ["Echo Ana Mar a RAM: TODO \"<otro$x>\" y Ana Mar"]);
        let (m, _) = correr(".KEY nombre\n.BRA {\n.KET }\nEcho {nombre$nadie} \"<nombre>\"", "");
        assert_eq!(m.hechas, ["Echo nadie \"<nombre>\""]);
        // Falta el obligatorio: el guion no corre.
        let (m, i) = correr(".KEY nombre/A\nEcho no", "");
        assert_eq!((m.hechas.len(), i.rc), (0, 20));
        assert_eq!(m.dicho, ["g: falta el argumento nombre"]);
    }

    #[test]
    fn variables_alias_y_quit() {
        let mut m = Falso::default();
        m.guiones.push(("g".into(), "Echo $saludo en $Idioma, $nada\nQuit 5\nEcho no".into()));
        let mut i = Interprete::nuevo();
        i.poner_local("Saludo", "hola");
        i.alias.push(("ls".into(), "List NOHEAD".into()));
        i.linea("Execute g");
        conducir(&mut i, &mut m, 100);
        i.linea("ls SYS:C ; con comentario");
        conducir(&mut i, &mut m, 100);
        assert_eq!(m.hechas, ["Echo hola en español, $nada", "List NOHEAD SYS:C"]);
        assert_eq!(i.rc, 0);
        // EndShell acaba el Shell con el último código.
        i.linea("Falla 5");
        conducir(&mut i, &mut m, 100);
        i.linea("EndShell");
        assert_eq!(conducir(&mut i, &mut m, 100).0, Paso::Fin(5));
    }

    #[test]
    fn lineas_hostiles() {
        let mut m = Falso::default();
        let mut i = Interprete::nuevo();
        for mala in ["Echo \"sin cerrar", "Echo >", "| Echo", &"x".repeat(MAX_LINEA + 1), "If a EQ a", "EndIf", "Skip x", "Execute", "Execute no-existe"] {
            i.linea(mala);
            let (p, _) = conducir(&mut i, &mut m, 100);
            assert_eq!(p, Paso::Pedir, "{mala}");
            assert!(i.rc >= 10, "{mala}");
        }
        assert!(m.hechas.is_empty());
        assert_eq!(m.dicho[0], "comillas sin cerrar");
        // Un guion con If sin EndIf, o con una línea larguísima, acaba con error y no se queda a medias.
        m.guiones = vec![("a".into(), "If EXISTS nada\nEcho no".into()), ("b".into(), format!("Echo antes\nEcho {}\nEcho no", "y".repeat(MAX_LINEA)))];
        i.linea("Execute a");
        assert_eq!(conducir(&mut i, &mut m, 100).0, Paso::Pedir);
        assert_eq!(m.dicho.last().map(String::as_str), Some("If sin su EndIf"));
        i.linea("Execute b");
        assert_eq!(conducir(&mut i, &mut m, 100).0, Paso::Pedir);
        assert_eq!(m.hechas, ["Echo antes"]);
        assert!(!i.ocupado());
        // Una variable que hace crecer la línea más de la cuenta.
        i.poner_local("gordo", &"z".repeat(1500));
        i.linea("Echo $gordo $gordo");
        assert_eq!(conducir(&mut i, &mut m, 100).0, Paso::Pedir);
        assert_eq!(m.dicho.last().map(String::as_str), Some("línea demasiado larga"));
    }
}
