//! Objetos al modo de MUI (K3): cada objeto tiene atributos que se ponen y se leen, y se puede pedir a un objeto que
//! avise cuando un atributo suyo cambie (`notificar`, como `MUIM_Notify`): entonces se pone un atributo de otro
//! objeto, con un valor fijo, el valor que cambió (`MUIV_TriggerValue`), su contrario o ese valor como texto.
//!
//! Escrito desde el modelo que describe la documentación pública de MUI, no desde su código (S11). Los avisos solo
//! saltan si el valor cambia de verdad: así dos objetos que se avisan el uno al otro no se quedan dando vueltas.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Un objeto: su número en el árbol.
pub type Obj = u32;

/// Un atributo: un número, como las etiquetas de MUI. Los de cada clase están en `atr`.
pub type Atr = u32;

/// Los atributos que conocen todas las clases (de 1 a 99) y los de cada una (desde 100).
pub mod atr {
    use super::Atr;
    /// Texto que enseña el objeto (un texto, la etiqueta de un botón).
    pub const TEXTO: Atr = 1;
    /// Desactivado: se ve apagado y no responde.
    pub const DESACTIVADO: Atr = 2;
    /// El valor de un control: el de un deslizador, una casilla, un ciclo.
    pub const VALOR: Atr = 3;
    /// Pulsado ahora mismo (un botón mientras se aprieta).
    pub const PULSADO: Atr = 4;
    /// Se soltó después de pulsar dentro: el "clic" de un botón.
    pub const CLIC: Atr = 5;
    /// Lo mínimo y lo máximo de un deslizador.
    pub const MINIMO: Atr = 6;
    pub const MAXIMO: Atr = 7;
    /// El peso de un objeto en su grupo (100 por defecto).
    pub const PESO: Atr = 8;
    /// Dónde está el cursor de un campo de texto, en letras desde el principio.
    pub const CURSOR: Atr = 9;
    /// Las opciones de un ciclo, separadas por '|' (como el array de textos de `MUIA_Cycle_Entries`).
    pub const OPCIONES: Atr = 10;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Valor {
    Int(i64),
    Bool(bool),
    Texto(String),
}

impl Valor {
    pub fn int(&self) -> i64 {
        match self {
            Valor::Int(v) => *v,
            Valor::Bool(b) => *b as i64,
            Valor::Texto(t) => t.parse().unwrap_or(0),
        }
    }

    pub fn bool(&self) -> bool {
        match self {
            Valor::Bool(b) => *b,
            Valor::Int(v) => *v != 0,
            Valor::Texto(t) => !t.is_empty(),
        }
    }

    pub fn texto(&self) -> String {
        match self {
            Valor::Texto(t) => t.clone(),
            Valor::Int(v) => v.to_string(),
            Valor::Bool(b) => (if *b { "sí" } else { "no" }).into(),
        }
    }
}

/// Cuándo salta un aviso.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cuando {
    /// Con cualquier valor nuevo (`MUIV_EveryTime`).
    Siempre,
    /// Cuando el valor nuevo es este.
    Igual(Valor),
}

/// Qué se pone en el destino.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Que {
    Fijo(Valor),
    /// El valor que cambió (`MUIV_TriggerValue`).
    Disparo,
    /// Su contrario, para un sí o no (`MUIV_NotTriggerValue`).
    NoDisparo,
    /// El valor que cambió, como texto, con `{}` sustituido por él (como `MUIM_SetAsString`).
    Formato(String),
}

struct Aviso {
    atr: Atr,
    cuando: Cuando,
    destino: Obj,
    destino_atr: Atr,
    que: Que,
}

/// Lo que hace una clase con sus atributos (qué acepta y cómo los guarda) y, si se ve, cómo mide, se dibuja y
/// responde al ratón (K5). Lo que no hace una clase, lo hace por defecto: nada.
pub trait Clase {
    /// El nombre de la clase, para depurar.
    fn nombre(&self) -> &'static str;
    /// Ajusta un valor antes de guardarlo (un deslizador lo deja entre su mínimo y su máximo). `None`: no lo acepta.
    fn ajustar(&self, _atr: Atr, valor: Valor, _attrs: &BTreeMap<Atr, Valor>) -> Option<Valor> {
        Some(valor)
    }
    /// Lo que mide.
    fn medida(&self, _attrs: &BTreeMap<Atr, Valor>, _ctx: &mut crate::controles::Contexto) -> crate::grupos::Medida {
        crate::grupos::Medida::fija(0, 0)
    }
    /// Se dibuja en `r`.
    fn dibujar(
        &self,
        _attrs: &BTreeMap<Atr, Valor>,
        _l: &mut crate::lienzo::Lienzo,
        _r: crate::lienzo::Rect,
        _ctx: &mut crate::controles::Contexto,
        _estado: crate::tema::Estado,
    ) {
    }
    /// Si se le puede pulsar.
    fn pulsable(&self) -> bool {
        false
    }
    /// El ratón pulsado o arrastrado en (x, y) de su rectángulo `r`: los atributos que cambian.
    fn arrastrar(&self, _attrs: &BTreeMap<Atr, Valor>, _r: crate::lienzo::Rect, _x: i32, _y: i32) -> Vec<(Atr, Valor)> {
        Vec::new()
    }
    /// Un clic: pulsar y soltar dentro.
    fn clic(&self, _attrs: &BTreeMap<Atr, Valor>) -> Vec<(Atr, Valor)> {
        Vec::new()
    }
    /// Si recibe el teclado cuando tiene el foco.
    fn usa_teclado(&self) -> bool {
        false
    }
    /// Una tecla, con el foco: los atributos que cambian.
    fn escribir(&self, _attrs: &BTreeMap<Atr, Valor>, _t: crate::teclado::Tecla) -> Vec<(Atr, Valor)> {
        Vec::new()
    }
}

pub struct Objeto {
    pub clase: Box<dyn Clase>,
    pub attrs: BTreeMap<Atr, Valor>,
    avisos: Vec<Aviso>,
    /// Hay que volver a dibujarlo.
    pub sucio: bool,
}

/// Todos los objetos de un programa.
#[derive(Default)]
pub struct Arbol {
    objetos: BTreeMap<Obj, Objeto>,
    siguiente: Obj,
    /// Para no dar vueltas sin fin si una cadena de avisos vuelve sobre sí misma con valores distintos cada vez.
    profundidad: u32,
}

/// Lo más hondo que se sigue una cadena de avisos.
pub const MAX_CADENA: u32 = 32;

impl Arbol {
    pub fn new() -> Self {
        Self::default()
    }

    /// Un objeto nuevo de `clase`, con sus atributos de partida.
    pub fn crear(&mut self, clase: impl Clase + 'static, attrs: &[(Atr, Valor)]) -> Obj {
        self.siguiente += 1;
        let id = self.siguiente;
        let mut o = Objeto { clase: Box::new(clase), attrs: BTreeMap::new(), avisos: Vec::new(), sucio: true };
        for (a, v) in attrs {
            if let Some(v) = o.clase.ajustar(*a, v.clone(), &o.attrs) {
                o.attrs.insert(*a, v);
            }
        }
        self.objetos.insert(id, o);
        id
    }

    pub fn objeto(&self, o: Obj) -> Option<&Objeto> {
        self.objetos.get(&o)
    }

    pub fn objeto_mut(&mut self, o: Obj) -> Option<&mut Objeto> {
        self.objetos.get_mut(&o)
    }

    pub fn leer(&self, o: Obj, a: Atr) -> Option<&Valor> {
        self.objetos.get(&o)?.attrs.get(&a)
    }

    /// Pone un atributo. Si cambia, el objeto queda por dibujar y saltan sus avisos.
    pub fn poner(&mut self, o: Obj, a: Atr, v: Valor) {
        let Some(obj) = self.objetos.get_mut(&o) else { return };
        let Some(v) = obj.clase.ajustar(a, v, &obj.attrs) else { return };
        if obj.attrs.get(&a) == Some(&v) {
            return;
        }
        obj.attrs.insert(a, v.clone());
        obj.sucio = true;
        if self.profundidad >= MAX_CADENA {
            return;
        }
        let disparos: Vec<(Obj, Atr, Valor)> = obj
            .avisos
            .iter()
            .filter(|n| n.atr == a && (n.cuando == Cuando::Siempre || n.cuando == Cuando::Igual(v.clone())))
            .map(|n| {
                let valor = match &n.que {
                    Que::Fijo(f) => f.clone(),
                    Que::Disparo => v.clone(),
                    Que::NoDisparo => Valor::Bool(!v.bool()),
                    Que::Formato(f) => Valor::Texto(f.replace("{}", &v.texto())),
                };
                (n.destino, n.destino_atr, valor)
            })
            .collect();
        self.profundidad += 1;
        for (d, da, dv) in disparos {
            self.poner(d, da, dv);
        }
        self.profundidad -= 1;
    }

    /// Que `o` avise cuando cambie su atributo `a`: entonces se pone `destino_atr` de `destino` (`MUIM_Notify`).
    pub fn notificar(&mut self, o: Obj, a: Atr, cuando: Cuando, destino: Obj, destino_atr: Atr, que: Que) {
        if let Some(obj) = self.objetos.get_mut(&o) {
            obj.avisos.push(Aviso { atr: a, cuando, destino, destino_atr, que });
        }
    }

    /// Los objetos que hay que volver a dibujar, sin limpiarlos.
    pub fn sucios_sin_limpiar(&self) -> Vec<Obj> {
        self.objetos.iter().filter(|(_, o)| o.sucio).map(|(&id, _)| id).collect()
    }

    /// Los objetos que hay que volver a dibujar, y los deja limpios.
    pub fn sucios(&mut self) -> Vec<Obj> {
        let mut out = Vec::new();
        for (&id, o) in self.objetos.iter_mut() {
            if o.sucio {
                o.sucio = false;
                out.push(id);
            }
        }
        out
    }
}

/// Una clase sin nada propio: guarda lo que le pongan.
pub struct Generica;

impl Clase for Generica {
    fn nombre(&self) -> &'static str {
        "genérica"
    }
}

/// Un deslizador: su `VALOR` queda siempre entre `MINIMO` y `MAXIMO`.
pub struct Deslizador;

impl Clase for Deslizador {
    fn nombre(&self) -> &'static str {
        "deslizador"
    }

    fn ajustar(&self, atr: Atr, valor: Valor, attrs: &BTreeMap<Atr, Valor>) -> Option<Valor> {
        if atr != atr::VALOR {
            return Some(valor);
        }
        let min = attrs.get(&atr::MINIMO).map_or(0, Valor::int);
        let max = attrs.get(&atr::MAXIMO).map_or(100, Valor::int);
        Some(Valor::Int(valor.int().clamp(min, max)))
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn un_deslizador_mueve_un_texto() {
        let mut t = Arbol::new();
        let d = t.crear(Deslizador, &[(atr::MINIMO, Valor::Int(0)), (atr::MAXIMO, Valor::Int(10))]);
        let texto = t.crear(Generica, &[(atr::TEXTO, Valor::Texto("nada".into()))]);
        t.notificar(d, atr::VALOR, Cuando::Siempre, texto, atr::TEXTO, Que::Formato("Volumen: {}".into()));
        t.sucios();
        t.poner(d, atr::VALOR, Valor::Int(7));
        assert_eq!(t.leer(texto, atr::TEXTO), Some(&Valor::Texto("Volumen: 7".into())));
        // Fuera de su rango, se queda en el borde (y el aviso lleva el valor ajustado).
        t.poner(d, atr::VALOR, Valor::Int(99));
        assert_eq!(t.leer(d, atr::VALOR), Some(&Valor::Int(10)));
        assert_eq!(t.leer(texto, atr::TEXTO), Some(&Valor::Texto("Volumen: 10".into())));
        assert_eq!(t.sucios(), [d, texto]);
    }

    #[test]
    fn avisos_con_valor_y_contrario() {
        let mut t = Arbol::new();
        let casilla = t.crear(Generica, &[(atr::VALOR, Valor::Bool(false))]);
        let boton = t.crear(Generica, &[]);
        let otro = t.crear(Generica, &[]);
        // Marcar la casilla desactiva el botón (y desmarcarla lo activa); solo al marcarla, el otro dice "marcada".
        t.notificar(casilla, atr::VALOR, Cuando::Siempre, boton, atr::DESACTIVADO, Que::Disparo);
        t.notificar(casilla, atr::VALOR, Cuando::Igual(Valor::Bool(true)), otro, atr::TEXTO, Que::Fijo(Valor::Texto("marcada".into())));
        t.notificar(casilla, atr::VALOR, Cuando::Siempre, otro, atr::VALOR, Que::NoDisparo);
        t.poner(casilla, atr::VALOR, Valor::Bool(true));
        assert_eq!(t.leer(boton, atr::DESACTIVADO), Some(&Valor::Bool(true)));
        assert_eq!(t.leer(otro, atr::TEXTO), Some(&Valor::Texto("marcada".into())));
        assert_eq!(t.leer(otro, atr::VALOR), Some(&Valor::Bool(false)));
        t.poner(casilla, atr::VALOR, Valor::Bool(false));
        assert_eq!(t.leer(boton, atr::DESACTIVADO), Some(&Valor::Bool(false)));
        assert_eq!(t.leer(otro, atr::TEXTO), Some(&Valor::Texto("marcada".into())));
    }

    #[test]
    fn dos_que_se_avisan_no_dan_vueltas() {
        // Dos deslizadores atados en los dos sentidos, como en MUI: mover uno mueve el otro, una sola vez.
        let mut t = Arbol::new();
        let a = t.crear(Deslizador, &[]);
        let b = t.crear(Deslizador, &[]);
        t.notificar(a, atr::VALOR, Cuando::Siempre, b, atr::VALOR, Que::Disparo);
        t.notificar(b, atr::VALOR, Cuando::Siempre, a, atr::VALOR, Que::Disparo);
        t.poner(a, atr::VALOR, Valor::Int(42));
        assert_eq!(t.leer(b, atr::VALOR), Some(&Valor::Int(42)));
        // Una cadena que cambia el valor cada vez (a = b + 1, b = a + 1, con formato) se corta en MAX_CADENA.
        let x = t.crear(Generica, &[]);
        let y = t.crear(Generica, &[]);
        t.notificar(x, atr::TEXTO, Cuando::Siempre, y, atr::TEXTO, Que::Formato("{}+".into()));
        t.notificar(y, atr::TEXTO, Cuando::Siempre, x, atr::TEXTO, Que::Formato("{}+".into()));
        t.poner(x, atr::TEXTO, Valor::Texto("".into()));
        t.poner(x, atr::TEXTO, Valor::Texto("a".into()));
        let largo = t.leer(x, atr::TEXTO).unwrap().texto().len();
        assert!(largo <= 2 * MAX_CADENA as usize, "{largo}");
    }
}
