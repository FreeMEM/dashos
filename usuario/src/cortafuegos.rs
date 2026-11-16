//! El cortafuegos de entrada del servicio de red (S13): cerrado de serie. Mira cada trama que llega para DashOS antes
//! de dársela a la pila y corta lo que nadie ha pedido:
//!
//! - un intento de conexión TCP (SYN sin ACK) a un puerto en el que nadie escucha;
//! - un datagrama UDP a un puerto que nadie tiene abierto.
//!
//! Los puertos abiertos los dice el servicio: los TCP en que escucha un programa con el permiso `red-escuchar`, y los
//! UDP de los programas, del DHCP y de las preguntas de DNS en curso. Lo cortado no se contesta (ni RST ni ICMP: quien
//! tantea no sabe si hay alguien) y se cuenta. Lo demás (ARP, ICMP, los segmentos de conexiones ya abiertas) pasa a la
//! pila, que tira lo que no es de nadie.
//!
//! En IPv6 se siguen las cabeceras de extensión corrientes hasta TCP o UDP; un fragmento, o una cadena más larga, pasa
//! a la pila, que no junta fragmentos (los tira).
//!
//! Sin `alloc` ni nada de DashOS: se prueba en el PC con `rustc --edition 2024 --test usuario/src/cortafuegos.rs`.

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Corte {
    /// SYN a un puerto TCP en el que nadie escucha.
    Tcp(u16),
    /// Datagrama a un puerto UDP que nadie tiene abierto.
    Udp(u16),
}

/// Los puertos abiertos ahora.
pub struct Abiertos<'a> {
    pub tcp: &'a [u16],
    pub udp: &'a [u16],
}

fn u16_en(t: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*t.get(i)?, *t.get(i + 1)?]))
}

/// Dónde empieza la cabecera de transporte de la trama y qué protocolo lleva (6 TCP, 17 UDP), si es IPv4 o IPv6 sin
/// fragmentar y se llega a ella.
fn transporte(t: &[u8]) -> Option<(u8, usize)> {
    match u16_en(t, 12)? {
        0x0800 => {
            let ihl = (*t.get(14)? & 0x0f) as usize * 4;
            // Un fragmento que no es el primero no trae la cabecera de transporte.
            if *t.get(14)? >> 4 != 4 || ihl < 20 || u16_en(t, 20)? & 0x1fff != 0 {
                return None;
            }
            Some((*t.get(23)?, 14 + ihl))
        }
        0x86dd => {
            if *t.get(14)? >> 4 != 6 {
                return None;
            }
            let (mut siguiente, mut p) = (*t.get(20)?, 54);
            // Salto a salto (0), encaminamiento (43) y opciones de destino (60): siguiente cabecera y largo en grupos
            // de 8 bytes, sin contar el primero.
            for _ in 0..4 {
                match siguiente {
                    0 | 43 | 60 => {
                        siguiente = *t.get(p)?;
                        p += (*t.get(p + 1)? as usize + 1) * 8;
                    }
                    _ => break,
                }
            }
            Some((siguiente, p))
        }
        _ => None,
    }
}

/// Si la trama `t` (para DashOS o de difusión) pasa a la pila.
pub fn entrada(t: &[u8], abiertos: &Abiertos) -> Result<(), Corte> {
    let Some((protocolo, p)) = transporte(t) else { return Ok(()) };
    match protocolo {
        6 => {
            let (Some(puerto), Some(&banderas)) = (u16_en(t, p + 2), t.get(p + 13)) else { return Ok(()) };
            // SYN sin ACK: alguien quiere entrar.
            if banderas & 0x12 == 0x02 && !abiertos.tcp.contains(&puerto) {
                return Err(Corte::Tcp(puerto));
            }
            Ok(())
        }
        17 => match u16_en(t, p + 2) {
            Some(puerto) if !abiertos.udp.contains(&puerto) => Err(Corte::Udp(puerto)),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn eth(tipo: u16, resto: &[u8]) -> Vec<u8> {
        let mut t = vec![0x52, 0x54, 0, 0x12, 0x34, 0x56, 2, 0, 0, 0, 0, 1];
        t.extend_from_slice(&tipo.to_be_bytes());
        t.extend_from_slice(resto);
        t
    }

    fn ipv4(protocolo: u8, opciones: usize, fragmento: u16, datos: &[u8]) -> Vec<u8> {
        let mut c = vec![0x40 | (5 + opciones as u8 / 4), 0, 0, 0, 0, 0, (fragmento >> 8) as u8, fragmento as u8, 64, protocolo, 0, 0, 192, 0, 2, 1, 10, 0, 2, 15];
        c.extend(core::iter::repeat_n(1u8, opciones));
        c.extend_from_slice(datos);
        eth(0x0800, &c)
    }

    fn ipv6(siguiente: u8, extensiones: &[(u8, usize)], datos: &[u8]) -> Vec<u8> {
        let mut c = vec![0x60, 0, 0, 0, 0, 0, siguiente, 64];
        c.extend_from_slice(&[0xfe; 16]);
        c.extend_from_slice(&[0xfd; 16]);
        for &(sig, grupos) in extensiones {
            c.extend_from_slice(&[sig, grupos as u8 - 1]);
            c.extend(core::iter::repeat_n(0u8, grupos * 8 - 2));
        }
        c.extend_from_slice(datos);
        eth(0x86dd, &c)
    }

    fn tcp(puerto: u16, banderas: u8) -> Vec<u8> {
        let mut s = vec![0x9c, 0x40, (puerto >> 8) as u8, puerto as u8, 0, 0, 0, 1, 0, 0, 0, 0, 0x50, banderas, 0xff, 0xff, 0, 0, 0, 0];
        s.extend_from_slice(b"hola");
        s
    }

    fn udp(puerto: u16) -> Vec<u8> {
        vec![0x9c, 0x40, (puerto >> 8) as u8, puerto as u8, 0, 12, 0, 0, 1, 2, 3, 4]
    }

    const SYN: u8 = 0x02;
    const ACK: u8 = 0x10;

    #[test]
    fn cerrado_de_serie() {
        let nada = Abiertos { tcp: &[], udp: &[] };
        let algo = Abiertos { tcp: &[7777], udp: &[68, 5000] };
        for hacer in [&(|p: u8, d: &[u8]| ipv4(p, 0, 0, d)) as &dyn Fn(u8, &[u8]) -> Vec<u8>, &|p, d| ipv4(p, 8, 0, d), &|p, d| ipv6(p, &[], d), &|p, d| ipv6(0, &[(60, 1), (43, 2), (p, 1)], d)] {
            // Entrar por TCP: solo donde se escucha.
            assert_eq!(entrada(&hacer(6, &tcp(7777, SYN)), &nada), Err(Corte::Tcp(7777)));
            assert_eq!(entrada(&hacer(6, &tcp(7777, SYN)), &algo), Ok(()));
            assert_eq!(entrada(&hacer(6, &tcp(22, SYN)), &algo), Err(Corte::Tcp(22)));
            // SYN con otras banderas raras (FIN, PSH, URG) sigue siendo un intento de entrar.
            assert_eq!(entrada(&hacer(6, &tcp(22, SYN | 0x29)), &algo), Err(Corte::Tcp(22)));
            // Lo de una conexión abierta desde dentro (SYN+ACK, ACK, datos, FIN, RST) pasa: lo mira la pila.
            for b in [SYN | ACK, ACK, ACK | 0x08, ACK | 0x01, 0x04] {
                assert_eq!(entrada(&hacer(6, &tcp(50000, b)), &nada), Ok(()));
            }
            // UDP: solo a un puerto abierto.
            assert_eq!(entrada(&hacer(17, &udp(5000)), &nada), Err(Corte::Udp(5000)));
            assert_eq!(entrada(&hacer(17, &udp(5000)), &algo), Ok(()));
            assert_eq!(entrada(&hacer(17, &udp(68)), &algo), Ok(()));
            assert_eq!(entrada(&hacer(17, &udp(5001)), &algo), Err(Corte::Udp(5001)));
            // ICMP e ICMPv6 pasan.
            assert_eq!(entrada(&hacer(1, &[8, 0, 0, 0]), &nada), Ok(()));
            assert_eq!(entrada(&hacer(58, &[134, 0, 0, 0]), &nada), Ok(()));
        }
        // ARP y tipos que no se conocen: a la pila.
        assert_eq!(entrada(&eth(0x0806, &[0; 28]), &nada), Ok(()));
        assert_eq!(entrada(&eth(0x88b5, &[0; 60]), &nada), Ok(()));
    }

    /// Hostil: tramas cortadas por cualquier sitio, fragmentos, cabeceras imposibles y bytes al azar: nunca revienta, y
    /// lo que no se entiende pasa a la pila (que lo tira), no abre nada.
    #[test]
    fn rotas() {
        let nada = Abiertos { tcp: &[], udp: &[] };
        for entera in [ipv4(6, 0, 0, &tcp(22, SYN)), ipv6(6, &[(6, 1)], &tcp(22, SYN)), ipv4(17, 0, 0, &udp(9))] {
            for n in 0..entera.len() {
                let _ = entrada(&entera[..n], &nada);
            }
        }
        // Un fragmento que no es el primero no trae puertos: pasa (la pila no junta fragmentos).
        assert_eq!(entrada(&ipv4(6, 0, 100, &tcp(22, SYN)), &nada), Ok(()));
        // El primer fragmento sí los trae.
        assert_eq!(entrada(&ipv4(6, 0, 0x2000, &tcp(22, SYN)), &nada), Err(Corte::Tcp(22)));
        assert_eq!(entrada(&ipv6(44, &[], &tcp(22, SYN)), &nada), Ok(()));
        // Versión que no casa con el tipo, cabecera de menos de 20 bytes.
        let mut t = ipv4(6, 0, 0, &tcp(22, SYN));
        t[14] = 0x65;
        assert_eq!(entrada(&t, &nada), Ok(()));
        t[14] = 0x43;
        assert_eq!(entrada(&t, &nada), Ok(()));
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut azar = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..200_000 {
            let mut t = if azar() % 2 == 0 { ipv4(6, 0, 0, &tcp(22, SYN)) } else { ipv6(0, &[(17, 1)], &udp(9)) };
            for _ in 0..1 + azar() % 6 {
                let i = (azar() % t.len() as u64) as usize;
                t[i] = azar() as u8;
            }
            t.truncate((azar() % (t.len() as u64 + 1)) as usize);
            let _ = entrada(&t, &nada);
        }
    }
}
