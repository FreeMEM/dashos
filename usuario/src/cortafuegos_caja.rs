//! S29: el cortafuegos de la caja Linux (`docs/diseño/S29-malware-linux-y-dashos.md`). El servicio de red hace de
//! puente de tramas entre la virtio-net del invitado y la tarjeta de verdad (H9): el invitado es un aparato más de la
//! red de casa, con su MAC. Sin más, un programa malo de la caja puede buscar y atacar los aparatos de la red local
//! (la impresora, el NAS, el router, el propio PC de quien prueba en QEMU), anunciarse como la puerta de enlace por
//! ARP para ver el tráfico de los demás, y recibir conexiones de la red local sin pasar por el cortafuegos de DashOS.
//!
//! Esto decide, trama a trama y sin estado, qué pasa (`Reglas::red_local` falso, lo de serie):
//!
//! - **Del invitado a la red**: ARP solo para preguntar por la puerta de enlace o el DNS (o anunciarse a sí mismo), y
//!   sin hacerse pasar por ellos; IPv4 a direcciones públicas, sí; a la red local (10/8, 172.16/12, 192.168/16, 169.254/16,
//!   100.64/10, 127/8, 0/8), solo el DNS (53) y DHCP (67) a la puerta de enlace o al DNS, y el ping a la puerta; la
//!   difusión, solo DHCP; la multidifusión, no (mDNS, SSDP: buscar aparatos). IPv6: a direcciones globales, sí; lo
//!   demás, solo el descubrimiento de vecinos y routers (ICMPv6 133 a 137) y DHCPv6. Otros tipos de trama, no.
//! - **De la red al invitado**: ARP, sí; IPv4 desde la red local, solo de la puerta de enlace o del DNS; desde fuera,
//!   sí (son respuestas: el router de casa no deja entrar lo demás); multidifusión, no. IPv6: desde direcciones
//!   globales y el descubrimiento de vecinos.
//!
//! Con `red_local` (un permiso que se preguntará como `red-local`, S6b), pasa todo, como antes. Es una primera pieza:
//! lo recomendado en S29 es pasar a traducción de direcciones (NAT) en el servicio de red, como dice S13.
//!
//! Sin dependencias: se prueba en el PC con `rustc --test`.

/// Lo que sabe el servicio de red de la red de casa (de su propio DHCP) y lo que se le permite a la caja.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reglas {
    pub puerta: Option<[u8; 4]>,
    pub dns: Option<[u8; 4]>,
    pub red_local: bool,
}

/// Por qué se corta una trama (para el registro y las pruebas).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corte {
    /// Un ARP que se hace pasar por la puerta de enlace o el DNS.
    ArpFalso,
    /// Un ARP que pregunta por otro aparato de la red local (buscar aparatos).
    ArpBuscar,
    /// IPv4 a la red local (o desde ella, sin ser la puerta ni el DNS).
    RedLocal,
    /// Multidifusión o difusión que no es DHCP.
    Multidifusion,
    /// IPv6 a o desde una dirección que no es global, fuera del descubrimiento de vecinos.
    Ipv6Local,
    /// Otro tipo de trama, o una mal formada.
    Otra,
}

impl Corte {
    pub fn texto(self) -> &'static str {
        match self {
            Corte::ArpFalso => "un ARP que se hace pasar por la puerta de enlace o el DNS",
            Corte::ArpBuscar => "un ARP que busca otro aparato de la red local",
            Corte::RedLocal => "IPv4 con la red local",
            Corte::Multidifusion => "multidifusión o difusión",
            Corte::Ipv6Local => "IPv6 que no es global",
            Corte::Otra => "una trama de otro tipo o mal formada",
        }
    }
}

const ARP: u16 = 0x0806;
const IPV4: u16 = 0x0800;
const IPV6: u16 = 0x86dd;
const UDP: u8 = 17;
const TCP: u8 = 6;
const ICMP: u8 = 1;
const ICMPV6: u8 = 58;

fn u16_en(t: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*t.get(i)?, *t.get(i + 1)?]))
}

fn ip4_en(t: &[u8], i: usize) -> Option<[u8; 4]> {
    t.get(i..i + 4)?.try_into().ok()
}

/// Una dirección IPv4 de la red local (o que no sale de ella).
pub fn ipv4_local(a: [u8; 4]) -> bool {
    matches!(a, [10, ..] | [127, ..] | [0, ..] | [169, 254, ..] | [192, 168, ..])
        || (a[0] == 172 && (16..32).contains(&a[1]))
        || (a[0] == 100 && (64..128).contains(&a[1]))
}

/// Multidifusión o difusión de IPv4.
fn ipv4_a_muchos(a: [u8; 4]) -> bool {
    (224..240).contains(&a[0]) || a == [255; 4]
}

/// Una dirección IPv6 global (2000::/3).
fn ipv6_global(a: &[u8]) -> bool {
    a.first().is_some_and(|b| b & 0xe0 == 0x20)
}

/// (protocolo, origen, destino, puerto de origen, puerto de destino) de un IPv4, con los puertos de TCP y UDP (0 en
/// los demás). `None` si está mal formado.
fn ipv4(t: &[u8]) -> Option<(u8, [u8; 4], [u8; 4], u16, u16)> {
    let ip = t.get(14..)?;
    let ihl = (*ip.first()? & 0x0f) as usize * 4;
    if *ip.first()? >> 4 != 4 || ihl < 20 {
        return None;
    }
    let proto = *ip.get(9)?;
    let (o, d) = (ip4_en(ip, 12)?, ip4_en(ip, 16)?);
    // Un fragmento que no es el primero no lleva puertos: cuenta como sin puertos.
    let primero = u16_en(ip, 6)? & 0x1fff == 0;
    let (po, pd) = if primero && (proto == TCP || proto == UDP) { (u16_en(ip, ihl)?, u16_en(ip, ihl + 2)?) } else { (0, 0) };
    Some((proto, o, d, po, pd))
}

/// Si una trama que sale del invitado puede ir a la red.
pub fn salida(t: &[u8], r: &Reglas) -> Result<(), Corte> {
    if r.red_local {
        return Ok(());
    }
    let tipo = u16_en(t, 12).ok_or(Corte::Otra)?;
    let es_de_casa = |a: [u8; 4]| Some(a) == r.puerta || Some(a) == r.dns;
    match tipo {
        ARP => {
            let (op, spa, tpa) = (u16_en(t, 20).ok_or(Corte::Otra)?, ip4_en(t, 28).ok_or(Corte::Otra)?, ip4_en(t, 38).ok_or(Corte::Otra)?);
            if es_de_casa(spa) {
                return Err(Corte::ArpFalso);
            }
            // Preguntar (1) por la puerta o el DNS, o anunciarse (la dirección que pregunta es la suya, o 0 al
            // comprobar si está libre). Contestar (2), sí: le preguntan por su dirección.
            match op {
                1 if es_de_casa(tpa) || spa == tpa || spa == [0; 4] => Ok(()),
                1 => Err(Corte::ArpBuscar),
                2 => Ok(()),
                _ => Err(Corte::Otra),
            }
        }
        IPV4 => {
            let (proto, _, d, po, pd) = ipv4(t).ok_or(Corte::Otra)?;
            if ipv4_a_muchos(d) {
                // Solo DHCP: el invitado pide su dirección.
                return if d == [255; 4] && proto == UDP && po == 68 && pd == 67 { Ok(()) } else { Err(Corte::Multidifusion) };
            }
            if !ipv4_local(d) {
                return Ok(());
            }
            // DHCP (el cliente, del 68 al 67) a quien le dio la dirección, sea quien sea.
            let dhcp = proto == UDP && po == 68 && pd == 67;
            let deja = dhcp || es_de_casa(d) && ((proto == UDP || proto == TCP) && pd == 53 || proto == ICMP && Some(d) == r.puerta);
            if deja { Ok(()) } else { Err(Corte::RedLocal) }
        }
        IPV6 => ipv6(t, true),
        _ => Err(Corte::Otra),
    }
}

/// IPv6 en un sentido (`sale`: del invitado).
fn ipv6(t: &[u8], sale: bool) -> Result<(), Corte> {
    let ip = t.get(14..54).ok_or(Corte::Otra)?;
    if ip[0] >> 4 != 6 {
        return Err(Corte::Otra);
    }
    let (siguiente, origen, destino) = (ip[6], &ip[8..24], &ip[24..40]);
    let otro = if sale { destino } else { origen };
    // El descubrimiento de vecinos y routers (sin cabeceras de extensión delante).
    if siguiente == ICMPV6 && t.get(54).is_some_and(|ty| (133..=137).contains(ty)) {
        return Ok(());
    }
    // DHCPv6: del invitado a los servidores (ff02::1:2, puerto 547), y su respuesta (al 546).
    if siguiente == UDP && u16_en(t, 56) == Some(if sale { 547 } else { 546 }) {
        return Ok(());
    }
    if ipv6_global(otro) && !(destino[0] == 0xff) { Ok(()) } else { Err(Corte::Ipv6Local) }
}

/// Si una trama de la red puede llegar al invitado (las que son para su MAC o de difusión).
pub fn entrada(t: &[u8], r: &Reglas) -> Result<(), Corte> {
    if r.red_local {
        return Ok(());
    }
    let tipo = u16_en(t, 12).ok_or(Corte::Otra)?;
    match tipo {
        ARP => Ok(()),
        IPV4 => {
            let (proto, o, d, po, pd) = ipv4(t).ok_or(Corte::Otra)?;
            if ipv4_a_muchos(d) {
                // La respuesta de DHCP puede llegar por difusión.
                return if proto == UDP && po == 67 && pd == 68 { Ok(()) } else { Err(Corte::Multidifusion) };
            }
            let dhcp = proto == UDP && po == 67 && pd == 68;
            if !ipv4_local(o) || Some(o) == r.puerta || Some(o) == r.dns || dhcp {
                Ok(())
            } else {
                Err(Corte::RedLocal)
            }
        }
        IPV6 => ipv6(t, false),
        _ => Err(Corte::Otra),
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x57];
    const CASA: Reglas = Reglas { puerta: Some([10, 0, 2, 2]), dns: Some([10, 0, 2, 3]), red_local: false };

    fn eth(tipo: u16, cuerpo: &[u8]) -> Vec<u8> {
        let mut t = vec![0xff; 6];
        t.extend_from_slice(&MAC);
        t.extend_from_slice(&tipo.to_be_bytes());
        t.extend_from_slice(cuerpo);
        t
    }

    fn arp(op: u16, spa: [u8; 4], tpa: [u8; 4]) -> Vec<u8> {
        let mut c = vec![0, 1, 8, 0, 6, 4];
        c.extend_from_slice(&op.to_be_bytes());
        c.extend_from_slice(&MAC);
        c.extend_from_slice(&spa);
        c.extend_from_slice(&[0; 6]);
        c.extend_from_slice(&tpa);
        eth(ARP, &c)
    }

    fn ip4(proto: u8, o: [u8; 4], d: [u8; 4], po: u16, pd: u16) -> Vec<u8> {
        let mut c = vec![0x45, 0, 0, 40, 0, 0, 0x40, 0, 64, proto, 0, 0];
        c.extend_from_slice(&o);
        c.extend_from_slice(&d);
        c.extend_from_slice(&po.to_be_bytes());
        c.extend_from_slice(&pd.to_be_bytes());
        c.extend_from_slice(&[0; 16]);
        eth(IPV4, &c)
    }

    fn ip6(siguiente: u8, o: [u8; 16], d: [u8; 16], resto: &[u8]) -> Vec<u8> {
        let mut c = vec![0x60, 0, 0, 0, 0, 8, siguiente, 64];
        c.extend_from_slice(&o);
        c.extend_from_slice(&d);
        c.extend_from_slice(resto);
        eth(IPV6, &c)
    }

    const YO: [u8; 4] = [10, 0, 2, 16];

    #[test]
    fn salida_de_serie() {
        // Internet y lo de casa que hace falta: sí.
        assert_eq!(salida(&ip4(TCP, YO, [93, 184, 216, 34], 40000, 443), &CASA), Ok(()));
        assert_eq!(salida(&ip4(UDP, YO, [10, 0, 2, 3], 40000, 53), &CASA), Ok(()));
        assert_eq!(salida(&ip4(TCP, YO, [10, 0, 2, 2], 40000, 53), &CASA), Ok(()));
        assert_eq!(salida(&ip4(UDP, [0; 4], [255; 4], 68, 67), &CASA), Ok(()));
        assert_eq!(salida(&ip4(UDP, YO, [10, 0, 2, 2], 68, 67), &CASA), Ok(()));
        assert_eq!(salida(&ip4(ICMP, YO, [10, 0, 2, 2], 0, 0), &CASA), Ok(()));
        assert_eq!(salida(&arp(1, YO, [10, 0, 2, 2]), &CASA), Ok(()));
        assert_eq!(salida(&arp(1, YO, YO), &CASA), Ok(()), "anunciarse");
        assert_eq!(salida(&arp(1, [0; 4], YO), &CASA), Ok(()), "ver si está libre");
        assert_eq!(salida(&arp(2, YO, [10, 0, 2, 2]), &CASA), Ok(()), "contestar");
        // Los puertos del anfitrión, otros aparatos de casa, buscar aparatos, mDNS y hacerse pasar por la puerta: no.
        assert_eq!(salida(&ip4(TCP, YO, [10, 0, 2, 2], 40000, 22), &CASA), Err(Corte::RedLocal));
        assert_eq!(salida(&ip4(TCP, YO, [192, 168, 1, 10], 40000, 445), &CASA), Err(Corte::RedLocal));
        assert_eq!(salida(&ip4(TCP, YO, [172, 20, 0, 1], 40000, 80), &CASA), Err(Corte::RedLocal));
        assert_eq!(salida(&ip4(ICMP, YO, [10, 0, 2, 3], 0, 0), &CASA), Err(Corte::RedLocal));
        assert_eq!(salida(&ip4(UDP, YO, [224, 0, 0, 251], 5353, 5353), &CASA), Err(Corte::Multidifusion));
        assert_eq!(salida(&ip4(UDP, YO, [255; 4], 9, 9), &CASA), Err(Corte::Multidifusion));
        assert_eq!(salida(&arp(1, YO, [10, 0, 2, 50]), &CASA), Err(Corte::ArpBuscar));
        assert_eq!(salida(&arp(2, [10, 0, 2, 2], YO), &CASA), Err(Corte::ArpFalso));
        assert_eq!(salida(&arp(1, [10, 0, 2, 3], [10, 0, 2, 3]), &CASA), Err(Corte::ArpFalso));
        assert_eq!(salida(&eth(0x88cc, &[0; 40]), &CASA), Err(Corte::Otra), "LLDP");
        // IPv6: global sí; vecinos sí; ULA y enlace local, no.
        let global = [0x20, 1, 0xd, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let enlace = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let ula = [0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        assert_eq!(salida(&ip6(TCP, enlace, global, &[0; 20]), &CASA), Ok(()));
        assert_eq!(salida(&ip6(ICMPV6, enlace, [0xff, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2], &[133, 0, 0, 0]), &CASA), Ok(()));
        assert_eq!(salida(&ip6(TCP, enlace, ula, &[0; 20]), &CASA), Err(Corte::Ipv6Local));
        assert_eq!(salida(&ip6(TCP, global, enlace, &[0; 20]), &CASA), Err(Corte::Ipv6Local));
        // Con el permiso de la red local, todo.
        let todo = Reglas { red_local: true, ..CASA };
        assert_eq!(salida(&ip4(TCP, YO, [192, 168, 1, 10], 40000, 445), &todo), Ok(()));
        assert_eq!(salida(&arp(2, [10, 0, 2, 2], YO), &todo), Ok(()));
    }

    #[test]
    fn entrada_de_serie() {
        assert_eq!(entrada(&arp(1, [10, 0, 2, 2], YO), &CASA), Ok(()));
        assert_eq!(entrada(&ip4(TCP, [93, 184, 216, 34], YO, 443, 40000), &CASA), Ok(()));
        assert_eq!(entrada(&ip4(UDP, [10, 0, 2, 3], YO, 53, 40000), &CASA), Ok(()));
        assert_eq!(entrada(&ip4(UDP, [10, 0, 2, 2], [255; 4], 67, 68), &CASA), Ok(()));
        // Antes de saber la puerta (el servicio aún sin DHCP), la respuesta de DHCP pasa igual.
        assert_eq!(entrada(&ip4(UDP, [10, 0, 2, 2], YO, 67, 68), &Reglas::default()), Ok(()));
        assert_eq!(salida(&ip4(UDP, YO, [10, 0, 2, 2], 68, 67), &Reglas::default()), Ok(()));
        // Un aparato de casa que llama al invitado, o mDNS: no.
        assert_eq!(entrada(&ip4(TCP, [192, 168, 1, 10], YO, 50000, 22), &CASA), Err(Corte::RedLocal));
        assert_eq!(entrada(&ip4(UDP, [10, 0, 2, 50], [224, 0, 0, 251], 5353, 5353), &CASA), Err(Corte::Multidifusion));
    }

    /// Hostil: tramas cortas, de cada largo, con cabeceras IPv4 que mienten (IHL de 0 o enorme): ni un pánico.
    #[test]
    fn tramas_rotas() {
        let base = ip4(TCP, YO, [10, 0, 2, 2], 1, 22);
        for n in 0..base.len() {
            let _ = salida(&base[..n], &CASA);
            let _ = entrada(&base[..n], &CASA);
        }
        for ihl in 0..16u8 {
            let mut t = base.clone();
            t[14] = 0x40 | ihl;
            let _ = salida(&t, &CASA);
        }
        let mut x = 12345u32;
        for _ in 0..20000 {
            let n = (x % 120) as usize;
            let t: Vec<u8> = (0..n)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    x as u8
                })
                .collect();
            let _ = salida(&t, &CASA);
            let _ = entrada(&t, &CASA);
        }
    }
}
