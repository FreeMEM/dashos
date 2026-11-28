//! La biblioteca de AHI: consultas, el camino de un reproductor de módulos (AHI_AllocAudio con PlayerFunc, y
//! SetSound/SetFreq/SetVol desde el gancho), SoundFunc, sonidos dinámicos, AHI_PlayA, efectos y lo hostil.

mod comun;

use caja_exec::Memoria;
use caja_sonido::ahi_lib::*;
use caja_sonido::*;
use comun::*;

const UNO: u32 = 0x10000;
const IMM: u32 = 1;
const M8S: u32 = 0;
const M16S: u32 = 1;

/// ahi.device abierto sin unidad (la biblioteca).
fn biblioteca(c: &mut Caja) -> u32 {
    let (_, ior, e) = c.abrir("ahi.device", AHI_NO_UNIT, AHIR_TAMANO, 4);
    assert_eq!(e, 0);
    c.e.mem.leer32(ior + caja_exec::io::IO_DEVICE).unwrap()
}

/// Una `struct Hook` con su `h_Entry` en un trozo de memoria propio (el "código" no corre: lo hace la prueba).
fn gancho(c: &mut Caja) -> u32 {
    let codigo = c.reservar(8);
    let h = c.reservar(20);
    c.e.mem.escribir32(h + 8, codigo).unwrap();
    h
}

fn alloc(c: &mut Caja, t: &[(u32, u32)]) -> u32 {
    let l = c.etiquetas(t);
    c.lib("AHI_AllocAudioA", &[], &[(1, l)])
}

fn control(c: &mut Caja, ctrl: u32, t: &[(u32, u32)]) -> u32 {
    let l = c.etiquetas(t);
    c.lib("AHI_ControlAudioA", &[], &[(2, ctrl), (1, l)])
}

/// AHI_LoadSound de un `AHISampleInfo` con estas muestras.
fn cargar(c: &mut Caja, ctrl: u32, sonido: u32, tipo: u32, formato: u32, dir: u32, largo: u32) -> u32 {
    let info = c.reservar(12);
    c.e.mem.escribir32(info, formato).unwrap();
    c.e.mem.escribir32(info + 4, dir).unwrap();
    c.e.mem.escribir32(info + 8, largo).unwrap();
    c.lib("AHI_LoadSound", &[sonido, tipo], &[(0, info), (2, ctrl)])
}

fn set_sound(c: &mut Caja, ctrl: u32, canal: u32, sonido: u32, desde: u32, largo: u32, fl: u32) {
    c.lib("AHI_SetSound", &[canal, sonido, desde, largo, fl], &[(2, ctrl)]);
}
fn set_freq(c: &mut Caja, ctrl: u32, canal: u32, hz: u32, fl: u32) {
    c.lib("AHI_SetFreq", &[canal, hz, fl], &[(2, ctrl)]);
}
fn set_vol(c: &mut Caja, ctrl: u32, canal: u32, vol: u32, pan: u32, fl: u32) {
    c.lib("AHI_SetVol", &[canal, vol, pan, fl], &[(2, ctrl)]);
}

#[test]
fn consultas_y_selector_sin_ventana() {
    let mut c = caja();
    assert_eq!(biblioteca(&mut c), c.s.base_ahi());
    assert_eq!(c.lib("AHI_NextAudioID", &[AHI_INVALID_ID], &[]), MODO);
    assert_eq!(c.lib("AHI_NextAudioID", &[MODO], &[]), AHI_INVALID_ID);
    let (nombre, f, canales, estereo, maxi, salida) =
        (c.reservar(64), c.reservar(4), c.reservar(4), c.reservar(4), c.reservar(4), c.reservar(16));
    let l = c.etiquetas(&[
        (AHIDB_BUFFERLEN, 64),
        (AHIDB_NAME, nombre),
        (AHIDB_FREQUENCYARG, 0),
        (AHIDB_FREQUENCY, f),
        (AHIDB_MAXCHANNELS, canales),
        (AHIDB_STEREO, estereo),
        (AHIDB_MAXPLAYSAMPLES, maxi),
        (AHIDB_OUTPUTARG, 0),
        (AHIDB_OUTPUT, salida),
    ]);
    assert_eq!(c.lib("AHI_GetAudioAttrsA", &[MODO], &[(2, 0), (1, l)]), 1);
    let n: Vec<u8> = (0..NOMBRE_MODO.len() as u32 + 1).map(|i| c.e.mem.leer8(nombre + i).unwrap()).collect();
    assert_eq!(&n[..NOMBRE_MODO.len()], NOMBRE_MODO);
    assert_eq!(n[NOMBRE_MODO.len()], 0);
    assert_eq!(c.e.mem.leer32(f).unwrap(), 48_000);
    assert_eq!(c.e.mem.leer32(canales).unwrap(), MAX_CANALES);
    assert_eq!(c.e.mem.leer32(estereo).unwrap(), 1);
    assert_eq!(c.e.mem.leer32(maxi).unwrap(), 1920 + TROZO as u32);
    assert_eq!(c.e.mem.leer8(salida).unwrap(), b'A');
    // El modo por defecto también; otro, no; sin modo ni AHIAudioCtrl, tampoco.
    assert_eq!(c.lib("AHI_GetAudioAttrsA", &[AHI_DEFAULT_ID], &[(2, 0), (1, l)]), 1);
    assert_eq!(c.lib("AHI_GetAudioAttrsA", &[0x0002_0004], &[(2, 0), (1, l)]), 0);
    assert_eq!(c.lib("AHI_GetAudioAttrsA", &[AHI_INVALID_ID], &[(2, 0), (1, l)]), 0);
    // Un búfer de nombre corto: recortado, con su 0.
    let corto = c.etiquetas(&[(AHIDB_BUFFERLEN, 6), (AHIDB_NAME, nombre)]);
    c.lib("AHI_GetAudioAttrsA", &[MODO], &[(2, 0), (1, corto)]);
    assert_eq!(c.e.mem.leer(nombre, 6).unwrap(), b"DashO\0");
    // AHI_BestAudioIDA.
    let bien = c.etiquetas(&[(AHIDB_STEREO, 1), (AHIDB_BITS, 16), (AHIDB_MAXCHANNELS, 32)]);
    assert_eq!(c.lib("AHI_BestAudioIDA", &[], &[(1, bien)]), MODO);
    let mal = c.etiquetas(&[(AHIDB_RECORD, 1)]);
    assert_eq!(c.lib("AHI_BestAudioIDA", &[], &[(1, mal)]), AHI_INVALID_ID);
    // El selector: sin ventana, el modo de la caja.
    let t = c.etiquetas(&[(AHIR_USERDATA, 7)]);
    let req = c.lib("AHI_AllocAudioRequestA", &[], &[(0, t)]);
    assert_ne!(req, 0);
    assert_eq!(c.e.mem.leer32(req).unwrap(), AHI_INVALID_ID);
    assert_eq!(c.lib("AHI_AudioRequestA", &[], &[(0, req), (1, 0)]), 1);
    assert_eq!((c.e.mem.leer32(req).unwrap(), c.e.mem.leer32(req + 4).unwrap()), (MODO, 48_000));
    assert_eq!(c.e.mem.leer32(req + 32).unwrap(), 7);
    assert_eq!(c.lib("AHI_AudioRequestA", &[], &[(0, 0), (1, 0)]), 0);
    c.lib("AHI_FreeAudioRequest", &[], &[(0, req)]);
    c.lib("AHI_FreeAudioRequest", &[], &[(0, 0)]);
    assert_eq!(c.lib("AHI_SampleFrameSize", &[3], &[]), 4);
    assert_eq!(c.lib("AHI_SampleFrameSize", &[10], &[]), 8);
    assert_eq!(c.lib("AHI_SampleFrameSize", &[99], &[]), 0);
    // AHIAudioCtrl: su ahiac_UserData, la frecuencia de mezcla (la pedida no importa) y sus atributos.
    let ctrl = alloc(&mut c, &[(AHIA_CHANNELS, 4), (AHIA_SOUNDS, 2), (AHIA_USERDATA, 0x1234), (AHIA_MIXFREQ, 22_050)]);
    assert_ne!(ctrl, 0);
    assert_eq!(c.e.mem.leer32(ctrl).unwrap(), 0x1234);
    let q = c.reservar(4);
    assert_eq!(control(&mut c, ctrl, &[(AHIC_MIXFREQ_QUERY, q)]), 0);
    assert_eq!(c.e.mem.leer32(q).unwrap(), 48_000);
    let id = c.reservar(4);
    let t = c.etiquetas(&[(AHIDB_AUDIOID, id)]);
    assert_eq!(c.lib("AHI_GetAudioAttrsA", &[AHI_INVALID_ID], &[(2, ctrl), (1, t)]), 1);
    assert_eq!(c.e.mem.leer32(id).unwrap(), MODO);
    // Grabar no se puede.
    assert_eq!(control(&mut c, ctrl, &[(AHIC_RECORD, 1)]), AHIE_UNKNOWN);
    c.lib("AHI_FreeAudio", &[], &[(2, ctrl)]);
    c.lib("AHI_FreeAudio", &[], &[(2, 0)]);
    c.sin_avisos();
    // Las privadas de la base de datos de modos: 0, con aviso.
    assert_eq!(c.lib("AHI_AddAudioMode", &[], &[(0, 0)]), 0);
    assert_eq!(c.avisos().len(), 1);
}

/// Un reproductor de módulos: 4 canales, el jugador a 50 Hz; en cada tic, el "programa" (aquí, Rust) cambia notas.
#[test]
fn reproductor_con_jugador_a_50_hz() {
    let mut c = caja();
    biblioteca(&mut c);
    let h = gancho(&mut c);
    let ctrl = alloc(
        &mut c,
        &[
            (AHIA_CHANNELS, 4),
            (AHIA_SOUNDS, 1),
            (AHIA_PLAYERFUNC, h),
            (AHIA_PLAYERFREQ, 50 << 16),
            (AHIA_MINPLAYERFREQ, 50 << 16),
            (AHIA_MAXPLAYERFREQ, 50 << 16),
        ],
    );
    let m = c.bytes(&[64; 100]);
    assert_eq!(cargar(&mut c, ctrl, 0, AHIST_SAMPLE, M8S, m, 100), 0);
    assert_eq!(control(&mut c, ctrl, &[(AHIC_PLAY, 1)]), 0);
    let mut tics = 0;
    c.pasar_con(1_000_000, 1_000, &mut |c, g| {
        assert_eq!((g.gancho, g.control, g.mensaje), (h, ctrl, 0));
        assert_eq!(g.entrada, c.e.mem.leer32(h + 8).unwrap());
        assert_eq!(g.registros(0x8000).a[2], ctrl);
        tics += 1;
        if tics == 1 {
            // El primer tic: la nota, a la izquierda, a todo volumen (entre 4 canales: 1/4).
            set_sound(c, ctrl, 0, 0, 0, 0, IMM);
            set_freq(c, ctrl, 0, 48_000, IMM);
            set_vol(c, ctrl, 0, UNO, 0, IMM);
        } else if tics == 11 {
            // Desde el tic 11 (el cuadro 9.600), a medio volumen. Sin IMM desde el jugador: al acabar la vuelta.
            set_vol(c, ctrl, 0, UNO / 2, 0, IMM);
        }
        // Un gancho que llama a atender (como lo haría una llamada nativa desde dentro): no mezcla por debajo.
        let antes = c.s.producidos();
        assert!(c.s.atender(&mut c.e, &mut c.salida, c.ahora, true).is_none());
        assert_eq!(c.s.producidos(), antes);
    });
    // 1 s de sonido (y lo que va por delante): 50 tics y uno o dos más.
    assert!((50..=53).contains(&tics), "{tics}");
    // Cada tic, en su cuadro exacto: cada 960.
    for (k, (_, p)) in c.ganchos.iter().enumerate() {
        assert_eq!(*p, k as u64 * 960, "tic {k}");
    }
    let s = &c.salida.escritos;
    assert!(s[..9600].iter().all(|x| *x == [4096, 0]), "{:?}", &s[..2]);
    assert!(s[9600..48_000].iter().all(|x| *x == [2048, 0]));
    // Parar: no más tics ni sonido.
    control(&mut c, ctrl, &[(AHIC_PLAY, 0)]);
    let (n, g) = (c.salida.escritos.len(), c.ganchos.len());
    c.pasar(100_000);
    assert_eq!((c.salida.escritos.len(), c.ganchos.len()), (n, g));
    assert!(!c.s.activo());
    c.sin_avisos();
}

#[test]
fn ritmo_entero_de_los_programas_viejos_y_cambio_de_ritmo() {
    let mut c = caja();
    biblioteca(&mut c);
    let h = gancho(&mut c);
    // 25 (sin desplazar) es 25 Hz, como dicen los autodocs en BUGS.
    let ctrl = alloc(&mut c, &[(AHIA_CHANNELS, 1), (AHIA_SOUNDS, 1), (AHIA_PLAYERFUNC, h), (AHIA_PLAYERFREQ, 25)]);
    control(&mut c, ctrl, &[(AHIC_PLAY, 1)]);
    c.pasar(1_000_000);
    let n = c.ganchos.len();
    assert!((25..=27).contains(&n), "{n}");
    // Cambiar el ritmo (desde un gancho se puede): a 100 Hz.
    control(&mut c, ctrl, &[(AHIA_PLAYERFREQ, 100 << 16)]);
    c.pasar(1_000_000);
    let m = c.ganchos.len() - n;
    assert!((99..=101).contains(&m), "{m}");
    c.sin_avisos();
}

#[test]
fn sin_poder_ejecutar_ganchos_la_mezcla_espera() {
    let mut c = caja();
    biblioteca(&mut c);
    let h = gancho(&mut c);
    let ctrl =
        alloc(&mut c, &[(AHIA_CHANNELS, 1), (AHIA_SOUNDS, 1), (AHIA_PLAYERFUNC, h), (AHIA_PLAYERFREQ, 50 << 16)]);
    control(&mut c, ctrl, &[(AHIC_PLAY, 1)]);
    c.ahora += 1_000;
    assert!(c.s.atender(&mut c.e, &mut c.salida, c.ahora, false).is_none());
    assert!(c.s.hay_gancho());
    assert_eq!(c.s.producidos(), 0, "no se mezcla más allá del tic sin su gancho");
    let g = c.s.atender(&mut c.e, &mut c.salida, c.ahora, true).unwrap();
    assert_eq!(g.gancho, h);
    // Sin gancho_hecho, nada se mezcla (un gancho en marcha)...
    assert!(c.s.atender(&mut c.e, &mut c.salida, c.ahora, true).is_none());
    assert_eq!(c.s.producidos(), 0);
    // ... salvo que pase más de un segundo: se da por perdido, con aviso.
    c.ahora += 1_500_000;
    c.atender_con(&mut |_, _| {});
    assert!(c.s.producidos() > 0);
    assert_eq!(c.avisos().len(), 1);
}

#[test]
fn soundfunc_en_cada_vuelta_y_el_siguiente_sonido() {
    let mut c = caja();
    biblioteca(&mut c);
    let h = gancho(&mut c);
    let ctrl = alloc(&mut c, &[(AHIA_CHANNELS, 4), (AHIA_SOUNDS, 2), (AHIA_SOUNDFUNC, h)]);
    let a = c.bytes(&[32; 480]);
    let b = c.bytes(&[-32i8 as u8; 480]);
    cargar(&mut c, ctrl, 0, AHIST_SAMPLE, M8S, a, 480);
    cargar(&mut c, ctrl, 1, AHIST_SAMPLE, M8S, b, 480);
    control(&mut c, ctrl, &[(AHIC_PLAY, 1)]);
    set_freq(&mut c, ctrl, 2, 48_000, IMM);
    set_vol(&mut c, ctrl, 2, UNO, UNO, IMM);
    set_sound(&mut c, ctrl, 2, 0, 0, 0, IMM);
    let mut vueltas = 0;
    c.pasar_con(100_000, 1_000, &mut |c, g| {
        assert_eq!(g.mensaje, ctrl + 4);
        assert_eq!(c.e.mem.leer16(g.mensaje).unwrap(), 2, "ahism_Channel");
        vueltas += 1;
        // Como el ejemplo de la guía: el siguiente, sin IMM (entra al acabar la vuelta).
        set_sound(c, ctrl, 2, 1, 0, 0, 0);
    });
    // 0,1 s son 10 vueltas de 480 cuadros (y las que van por delante).
    assert!((10..=15).contains(&vueltas), "{vueltas}");
    let s = &c.salida.escritos;
    // La primera vuelta con el sonido 0 y desde ahí el 1 (a la derecha, entre 4 canales).
    assert!(s[..480].iter().all(|x| *x == [0, 32 << 8 >> 2]));
    assert!(s[480..4800].iter().all(|x| *x == [0, -32 << 8 >> 2]));
    c.sin_avisos();
}

#[test]
fn sonido_dinamico_se_lee_al_mezclar() {
    let mut c = caja();
    biblioteca(&mut c);
    let ctrl = alloc(&mut c, &[(AHIA_CHANNELS, 1), (AHIA_SOUNDS, 1)]);
    let m = c.bytes(&[0x10, 0x00].repeat(4800));
    assert_eq!(cargar(&mut c, ctrl, 0, AHIST_DYNAMICSAMPLE, M16S, m, 4800), 0);
    control(&mut c, ctrl, &[(AHIC_PLAY, 1)]);
    set_freq(&mut c, ctrl, 0, 48_000, IMM);
    set_vol(&mut c, ctrl, 0, UNO, 0, IMM);
    set_sound(&mut c, ctrl, 0, 0, 0, 0, IMM);
    c.pasar(20_000);
    // El programa reescribe su búfer: lo que se mezcla desde ahora es lo nuevo (sin copia guardada).
    let ya = c.salida.escritos.len();
    c.e.mem.leer_mut(m, 9600).unwrap().copy_from_slice(&[0x20, 0x00].repeat(4800));
    c.pasar(20_000);
    let s = &c.salida.escritos;
    assert!(s[..ya].iter().all(|x| *x == [0x1000, 0]));
    assert!(s[ya..s.len().min(4800)].iter().all(|x| *x == [0x2000, 0]));
    c.sin_avisos();
}

#[test]
fn play_a_con_bucle_y_volumen_maestro() {
    let mut c = caja();
    biblioteca(&mut c);
    let ctrl = alloc(&mut c, &[(AHIA_CHANNELS, 2), (AHIA_SOUNDS, 2)]);
    let a = c.bytes(&[64; 240]);
    let b = c.bytes(&[16; 240]);
    cargar(&mut c, ctrl, 0, AHIST_SAMPLE, M8S, a, 240);
    cargar(&mut c, ctrl, 1, AHIST_SAMPLE, M8S, b, 240);
    control(&mut c, ctrl, &[(AHIC_PLAY, 1)]);
    // Canal 1: el sonido 0 una vez y luego el 1 en bucle, a la izquierda.
    let t = c.etiquetas(&[
        (AHIP_BEGINCHANNEL, 1),
        (AHIP_FREQ, 48_000),
        (AHIP_VOL, UNO),
        (AHIP_PAN, 0),
        (AHIP_SOUND, 0),
        (AHIP_LOOPSOUND, 1),
        (AHIP_ENDCHANNEL, 0),
    ]);
    c.lib("AHI_PlayA", &[], &[(2, ctrl), (1, t)]);
    c.pasar(10_000);
    let s = c.salida.escritos.clone();
    assert!(s[..240].iter().all(|x| *x == [64 << 8 >> 1, 0]));
    assert!(s[240..480].iter().all(|x| *x == [16 << 8 >> 1, 0]));
    // El volumen maestro a 2,0 (el máximo con 2 canales): sin la división entre canales.
    let ef = c.reservar(8);
    c.e.mem.escribir32(ef, AHIET_MASTERVOLUME).unwrap();
    c.e.mem.escribir32(ef + 4, 2 * UNO).unwrap();
    assert_eq!(c.lib("AHI_SetEffect", &[], &[(0, ef), (2, ctrl)]), 0);
    let n = c.salida.escritos.len();
    c.pasar(10_000);
    assert_eq!(c.salida.escritos[n], [16 << 8, 0]);
    // Quitarlo (AHIET_CANCEL), y un efecto que no se hace: AHIE_UNKNOWN y aviso.
    c.e.mem.escribir32(ef, AHIET_MASTERVOLUME | AHIET_CANCEL).unwrap();
    assert_eq!(c.lib("AHI_SetEffect", &[], &[(0, ef), (2, ctrl)]), 0);
    c.sin_avisos();
    c.e.mem.escribir32(ef, 2).unwrap();
    assert_eq!(c.lib("AHI_SetEffect", &[], &[(0, ef), (2, ctrl)]), AHIE_UNKNOWN);
    assert_eq!(c.avisos().len(), 1);
}

#[test]
fn hostil() {
    let mut c = caja();
    biblioteca(&mut c);
    // Canales o sonidos 0 o absurdos, sin etiquetas: 0, con aviso.
    for t in [
        vec![(AHIA_CHANNELS, 0), (AHIA_SOUNDS, 1)],
        vec![(AHIA_CHANNELS, 100_000), (AHIA_SOUNDS, 1)],
        vec![(AHIA_CHANNELS, 1), (AHIA_SOUNDS, 0xFFFF_FFFF)],
        vec![],
    ] {
        assert_eq!(alloc(&mut c, &t), 0);
    }
    assert_eq!(c.lib("AHI_AllocAudioA", &[], &[(1, 0)]), 0);
    assert_eq!(c.lib("AHI_AllocAudioA", &[], &[(1, 0x10)]), 0);
    // Una lista de etiquetas en círculo (TAG_MORE a sí misma): aviso, sin colgarse.
    let circulo = c.reservar(8);
    c.e.mem.escribir32(circulo, 2).unwrap();
    c.e.mem.escribir32(circulo + 4, circulo).unwrap();
    assert_eq!(c.lib("AHI_AllocAudioA", &[], &[(1, circulo)]), 0);
    assert!(c.avisos().len() >= 6);
    let ctrl = alloc(&mut c, &[(AHIA_CHANNELS, 2), (AHIA_SOUNDS, 2)]);
    // Una AHIAudioCtrl que no lo es (otra memoria, la página 0, 0): aviso y nada.
    for x in [0u32, 0x100, ctrl + 4, 0xFFFF_FFFC] {
        set_vol(&mut c, x, 0, UNO, 0, IMM);
        control(&mut c, x, &[(AHIC_PLAY, 1)]);
        assert_eq!(c.lib("AHI_LoadSound", &[0, 0, 0], &[(0, 0), (2, x)]), 0);
    }
    assert!(c.avisos().len() >= 12);
    // Canales fuera de rango.
    set_vol(&mut c, ctrl, 2, UNO, 0, IMM);
    set_freq(&mut c, ctrl, 0xFFFF, 1, IMM);
    set_sound(&mut c, ctrl, 7, 0, 0, 0, IMM);
    assert_eq!(c.avisos().len(), 3);
    // Sonidos: tipo malo, formato malo, en la página 0, fuera de la memoria, enormes, número fuera.
    let m = c.bytes(&[1; 64]);
    assert_eq!(cargar(&mut c, ctrl, 0, 7, M8S, m, 64), AHIE_BADSOUNDTYPE);
    assert_eq!(cargar(&mut c, ctrl, 0, AHIST_SAMPLE, 77, m, 64), AHIE_BADSAMPLETYPE);
    assert_eq!(cargar(&mut c, ctrl, 0, AHIST_SAMPLE, M8S, 0x40, 64), AHIE_UNKNOWN);
    assert_eq!(cargar(&mut c, ctrl, 0, AHIST_SAMPLE, M8S, TAMANO - 8, 64), AHIE_UNKNOWN);
    assert_eq!(cargar(&mut c, ctrl, 0, AHIST_SAMPLE, M16S, m, 0x9000_0000), AHIE_UNKNOWN);
    assert_eq!(cargar(&mut c, ctrl, 0, AHIST_SAMPLE, M8S, m, 0), AHIE_UNKNOWN);
    assert_eq!(cargar(&mut c, ctrl, 9, AHIST_SAMPLE, M8S, m, 64), AHIE_UNKNOWN);
    assert_eq!(c.lib("AHI_LoadSound", &[0, 0], &[(0, 0x20), (2, ctrl)]), 0, "info en la página 0");
    assert_eq!(c.avisos().len(), 6);
    // El dinámico de "toda la memoria" (dirección 0, largo 0xFFFFFFFF) vale con 8 bits: SetSound lleva la dirección.
    assert_eq!(cargar(&mut c, ctrl, 1, AHIST_DYNAMICSAMPLE, M8S, 0, 0xFFFF_FFFF), 0);
    // Un sonido sin cargar, un tramo fuera: aviso, el canal sigue como estaba.
    set_sound(&mut c, ctrl, 0, 0, 0, 0, IMM);
    assert_eq!(cargar(&mut c, ctrl, 0, AHIST_SAMPLE, M8S, m, 64), 0);
    set_sound(&mut c, ctrl, 0, 0, 60, 10, IMM);
    set_sound(&mut c, ctrl, 0, 0, 0, -10i32 as u32, IMM);
    assert_eq!(c.avisos().len(), 3);
    // Frecuencias absurdas: recortadas, con aviso; 0 para; AHI_MIXFREQ es la de la mezcla.
    set_freq(&mut c, ctrl, 0, 0xFFFF_FFFF - 1, IMM);
    assert_eq!(c.avisos().len(), 1);
    set_freq(&mut c, ctrl, 0, 0, IMM);
    set_freq(&mut c, ctrl, 0, AHI_MIXFREQ, IMM);
    // Volúmenes y paneos absurdos: recortados.
    set_vol(&mut c, ctrl, 0, 0x7FFF_FFFF, 0x8000_0000, IMM);
    // Sonando el dinámico de toda la memoria por la página 0 y por el final: silencio, sin pánico.
    control(&mut c, ctrl, &[(AHIC_PLAY, 1)]);
    set_sound(&mut c, ctrl, 1, 1, 0, 100, IMM);
    set_freq(&mut c, ctrl, 1, 48_000, IMM);
    set_vol(&mut c, ctrl, 1, UNO, 0, IMM);
    c.pasar(5_000);
    set_sound(&mut c, ctrl, 1, 1, 0xFFFF_FF00, 0, IMM);
    c.pasar(5_000);
    // Descargar lo que suena: se calla. Descargar lo que no existe: nada.
    c.lib("AHI_UnloadSound", &[1], &[(2, ctrl)]);
    c.lib("AHI_UnloadSound", &[999], &[(2, ctrl)]);
    // Consultas a punteros malos: aviso y AHIE_UNKNOWN; GetAudioAttrs, FALSE.
    assert_eq!(control(&mut c, ctrl, &[(AHIC_MIXFREQ_QUERY, 0x10)]), AHIE_UNKNOWN);
    let t = c.etiquetas(&[(AHIDB_FREQUENCY, 0xFFFF_FFF0), (AHIDB_BUFFERLEN, 0xFFFF_FFFF), (AHIDB_NAME, 0)]);
    assert_eq!(c.lib("AHI_GetAudioAttrsA", &[MODO], &[(2, 0), (1, t)]), 0);
    assert_eq!(c.avisos().len(), 3);
    // Ganchos malos: h_Entry 0, impar, fuera; el gancho en la página 0. Aviso y se sigue mezclando.
    let malos = [0u32, 0x1001, 0xFFFF_FF00];
    for e in malos {
        let h = c.reservar(20);
        c.e.mem.escribir32(h + 8, e).unwrap();
        control(&mut c, ctrl, &[(AHIA_PLAYERFUNC, h), (AHIA_PLAYERFREQ, 50 << 16)]);
        c.pasar(30_000);
    }
    control(&mut c, ctrl, &[(AHIA_PLAYERFUNC, 0x20)]);
    let antes = c.s.producidos();
    c.pasar(30_000);
    assert!(c.s.producidos() > antes);
    assert!(c.ganchos.is_empty());
    assert!(c.avisos().len() >= 4);
    // AHI_FreeAudio dos veces: la segunda, aviso. AHI_FreeAudioRequest de algo que no es un selector, aviso.
    c.lib("AHI_FreeAudio", &[], &[(2, ctrl)]);
    c.lib("AHI_FreeAudio", &[], &[(2, ctrl)]);
    c.lib("AHI_FreeAudioRequest", &[], &[(0, 0x5000)]);
    assert_eq!(c.avisos().len(), 2);
    assert!(!c.s.activo());
}

#[test]
fn tarea_que_muere_y_cerrar_sin_freeaudio() {
    let mut c = caja();
    let t2 = c.e.crear_tarea("reproductor", 0, 4096, 0x10_0000).unwrap();
    c.e.poner_en_marcha(t2).unwrap();
    let (_, ior, _) = c.abrir("ahi.device", AHI_NO_UNIT, AHIR_TAMANO, 4);
    let h = gancho(&mut c);
    let ctrl =
        alloc(&mut c, &[(AHIA_CHANNELS, 1), (AHIA_SOUNDS, 1), (AHIA_PLAYERFUNC, h), (AHIA_PLAYERFREQ, 50 << 16)]);
    control(&mut c, ctrl, &[(AHIC_PLAY, 1)]);
    c.pasar(50_000);
    assert!(!c.ganchos.is_empty() && c.s.activo());
    // La tarea muere: su AHIAudioCtrl deja de sonar y no hay más ganchos.
    c.s.tarea_muerta(t2);
    c.e.poner_en_marcha(c.tarea).unwrap();
    c.e.matar_tarea(t2).unwrap();
    let n = c.ganchos.len();
    c.pasar(100_000);
    assert_eq!(c.ganchos.len(), n);
    assert!(!c.s.activo());
    // Cerrar ahi.device sin AHI_FreeAudio: se suelta, con aviso.
    let (_, i2, _) = c.abrir("ahi.device", AHI_NO_UNIT, AHIR_TAMANO, 4);
    let ctrl = alloc(&mut c, &[(AHIA_CHANNELS, 1), (AHIA_SOUNDS, 1)]);
    control(&mut c, ctrl, &[(AHIC_PLAY, 1)]);
    c.sin_avisos();
    c.io("CloseDevice", i2);
    assert_eq!(c.avisos().len(), 1);
    assert!(!c.s.activo());
    let _ = ior;
}

/// Lo que cuesta mezclar 20 ms con 8 canales (dinámicos de 16 bits estéreo, cada uno a su frecuencia).
#[test]
fn rendimiento_ocho_canales() {
    let mut c = caja();
    biblioteca(&mut c);
    let ctrl = alloc(&mut c, &[(AHIA_CHANNELS, 8), (AHIA_SOUNDS, 8)]);
    for k in 0..8u32 {
        let b: Vec<u8> = (0..48_000u32)
            .flat_map(|i| [((i * (k + 1)) as i16).to_be_bytes(), ((i * 3) as i16).to_be_bytes()].concat())
            .collect();
        let m = c.bytes(&b);
        cargar(&mut c, ctrl, k, AHIST_DYNAMICSAMPLE, 3, m, 48_000);
        set_freq(&mut c, ctrl, k, 22_050 + 3_000 * k, IMM);
        set_vol(&mut c, ctrl, k, UNO, k * UNO / 8, IMM);
        set_sound(&mut c, ctrl, k, k, 0, 0, IMM);
    }
    control(&mut c, ctrl, &[(AHIC_PLAY, 1)]);
    c.pasar(40_000);
    let veces = 100;
    let t = std::time::Instant::now();
    for _ in 0..veces {
        c.ahora += 20_000;
        c.s.atender(&mut c.e, &mut c.salida, c.ahora, true);
    }
    let por_trozo = t.elapsed() / veces;
    eprintln!("mezclar 20 ms con 8 canales: {:?} ({} cuadros)", por_trozo, c.salida.escritos.len());
    assert!(c.salida.escritos.len() >= 960 * veces as usize);
    assert!(por_trozo < std::time::Duration::from_millis(20), "más lento que el tiempo real: {por_trozo:?}");
}
