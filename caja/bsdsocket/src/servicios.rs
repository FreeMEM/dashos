//! Nombres y direcciones: `gethostbyname` (y `_r`), `gethostbyaddr` (y `_r`), `getservby*`, `getprotoby*`,
//! `getnetby*`, los recorridos `get*ent`, `Inet_NtoA`, `inet_*`, `gethostname`, `gethostid`, `vsyslog`,
//! `getaddrinfo`, `freeaddrinfo`, `gai_strerror` y `getnameinfo`, de sus autodocs. Lo que no está hecho, aquí cae en
//! `Mal::NoHecha`.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use caja_exec::estructuras::memf;
use caja_exec::{Exec, Fallo, Memoria};

use crate::definiciones::errno::*;
use crate::definiciones::{addrinfo, hostent, netdb, netent, privado, protoent, servent, sock, sockaddr_in};
use crate::nombres;
use crate::red::{ErrorRed, Red};
use crate::*;

/// Dónde se escribe una `hostent` y lo suyo.
struct Zona {
    hp: u32,
    alias: u32,
    lista: u32,
    dirs: u32,
    nombre: u32,
    max_nombre: u32,
}

fn llenar_hostent<M: Memoria>(ex: &mut Exec<M>, z: &Zona, nombre: &[u8], dirs: &[[u8; 4]]) -> Res<()> {
    e32(ex, z.alias, 0)?;
    for (i, d) in dirs.iter().enumerate() {
        let i = i as u32;
        escribir_bytes(ex, z.dirs + 4 * i, d)?;
        e32(ex, z.lista + 4 * i, z.dirs + 4 * i)?;
    }
    e32(ex, z.lista + 4 * dirs.len() as u32, 0)?;
    poner_cadena(ex, z.nombre, nombre, z.max_nombre)?;
    e32(ex, z.hp + hostent::H_NAME, z.nombre)?;
    e32(ex, z.hp + hostent::H_ALIASES, z.alias)?;
    e32(ex, z.hp + hostent::H_ADDRTYPE, sock::AF_INET)?;
    e32(ex, z.hp + hostent::H_LENGTH, 4)?;
    e32(ex, z.hp + hostent::H_ADDR_LIST, z.lista)?;
    Ok(())
}

/// Lo que da `resolver`: las direcciones, el error del servicio, o que la tarea espera.
enum Nombre {
    Hecho(Result<Vec<[u8; 4]>, ErrorRed>),
    Espera(Hecho),
}

/// Los argumentos de `vsyslog` (LONG en la pila, `__stdargs`), con `%s`, `%d`, `%u`, `%x`, `%c`, `%m` y `%%`.
fn formatear<M: Memoria>(ex: &Exec<M>, f: &[u8], args: u32, errno: i32) -> String {
    let mut s = String::new();
    let mut i = 0;
    let mut k = 0u32;
    let mut sig = |ex: &Exec<M>| -> u32 {
        let v = if args != 0 { l32(ex, args + 4 * k).unwrap_or(0) } else { 0 };
        k += 1;
        v
    };
    while i < f.len() && s.len() < 512 {
        let c = f[i];
        i += 1;
        if c != b'%' {
            s.push(c as char);
            continue;
        }
        while i < f.len() && (f[i] == b'l' || f[i] == b'-' || f[i].is_ascii_digit()) {
            i += 1;
        }
        let Some(&t) = f.get(i) else { break };
        i += 1;
        match t {
            b'%' => s.push('%'),
            b'd' | b'i' => s.push_str(&format!("{}", sig(ex) as i32)),
            b'u' => s.push_str(&format!("{}", sig(ex))),
            b'x' => s.push_str(&format!("{:x}", sig(ex))),
            b'X' => s.push_str(&format!("{:X}", sig(ex))),
            b'c' => s.push(sig(ex) as u8 as char),
            b's' => {
                let p = sig(ex);
                let t = cadena(ex, p, 256).unwrap_or_default();
                s.extend(t.iter().map(|&c| c as char));
            }
            b'm' => s.push_str(nombres::texto_errno(errno)),
            _ => {}
        }
    }
    s
}

impl Bsdsocket {
    pub(crate) fn servicio<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        ll: &Llamada,
        nombre: &'static str,
    ) -> Res<Hecho> {
        let (d, a, base) = (ll.d, ll.a, ll.base);
        let v = |x: u32| Ok(Hecho::Valor(x));
        match nombre {
            "Inet_NtoA" => {
                let p = base + privado::NTOA;
                poner_cadena(ex, p, &nombres::ntoa(d[0]), 16)?;
                v(p)
            }
            "inet_addr" => v(nombres::inet_aton(&cadena(ex, a[0], 256)?).unwrap_or(sock::INADDR_NONE)),
            "inet_network" => v(nombres::inet_network(&cadena(ex, a[0], 256)?).unwrap_or(sock::INADDR_NONE)),
            "Inet_LnaOf" => v(nombres::lnaof(d[0])),
            "Inet_NetOf" => v(nombres::netof(d[0])),
            "Inet_MakeAddr" => v(nombres::makeaddr(d[0], d[1])),
            "inet_aton" => match nombres::inet_aton(&cadena(ex, a[0], 256)?) {
                Some(ip) => {
                    if a[1] != 0 {
                        e32(ex, a[1], ip)?;
                    }
                    v(1)
                }
                None => v(0),
            },
            "inet_ntop" => {
                if d[0] != sock::AF_INET {
                    return errno(EAFNOSUPPORT);
                }
                let ip = l32(ex, a[0])?;
                let t = nombres::ntoa(ip);
                if (d[1] as i32) < t.len() as i32 + 1 {
                    return errno(ENOSPC);
                }
                poner_cadena(ex, a[1], &t, d[1])?;
                v(a[1])
            }
            "inet_pton" => {
                if d[0] != sock::AF_INET {
                    return errno(EAFNOSUPPORT);
                }
                match nombres::inet_pton4(&cadena(ex, a[0], 256)?) {
                    Some(ip) => {
                        e32(ex, a[1], ip)?;
                        v(1)
                    }
                    None => v(0),
                }
            }
            "In_LocalAddr" => {
                let ip = d[0].to_be_bytes();
                v((ip[0] == 127 || (ip == red.direccion_local() && ip != [0; 4])) as u32)
            }
            "In_CanForward" | "ProcessIsServer" => v(0),
            "GetDefaultDomainName" => {
                if a[0] != 0 && d[0] > 0 {
                    e8(ex, a[0], 0)?;
                }
                v(0)
            }
            "gethostname" => {
                let n = d[0] as i32;
                if n <= 0 {
                    return errno(EINVAL);
                }
                let nombre = red.nombre_maquina();
                let k = nombre.len().min(n as usize);
                tramo(ex, a[0], k as u32, true)?;
                escribir_bytes(ex, a[0], &nombre[..k])?;
                if k < n as usize {
                    e8(ex, a[0] + k as u32, 0)?;
                }
                v(0)
            }
            "gethostid" => {
                let ip = red.direccion_local();
                v(if ip == [0; 4] { 0x7F00_0001 } else { u32::from_be_bytes(ip) })
            }
            "vsyslog" => {
                let b = self.base_ref(base);
                let (mascara, etiqueta, e) = (b.log[3], b.log[1], b.errno);
                if (1u32 << (d[0] & 7)) & mascara != 0 {
                    let f = cadena(ex, a[0], 1024)?;
                    let mut s = formatear(ex, &f, a[1], e);
                    if etiqueta != 0 {
                        let t = cadena(ex, etiqueta, 64).unwrap_or_default();
                        let mut p: String = t.iter().map(|&c| c as char).collect();
                        p.push_str(": ");
                        s.insert_str(0, &p);
                    }
                    self.anotar(s);
                }
                Ok(Hecho::Nada)
            }
            "gethostbyname" => self.gethostbyname(ex, red, ll, None),
            "gethostbyname_r" => self.gethostbyname(ex, red, ll, Some((a[1], a[2], d[0], a[3]))),
            "gethostbyaddr" => self.gethostbyaddr(ex, red, base, a[0], d[0], d[1], None),
            "gethostbyaddr_r" => self.gethostbyaddr(ex, red, base, a[0], d[0], d[1], Some((a[1], a[2], d[2], a[3]))),
            "getservbyname" => {
                let n = cadena(ex, a[0], 256)?;
                let p = if a[1] != 0 { Some(cadena(ex, a[1], 32)?) } else { None };
                let s = nombres::servicio_por_nombre(&n, p.as_deref());
                v(self.poner_servent(ex, base, s)?)
            }
            "getservbyport" => {
                let p = if a[0] != 0 { Some(cadena(ex, a[0], 32)?) } else { None };
                let s = nombres::servicio_por_puerto(d[0] as u16, p.as_deref());
                v(self.poner_servent(ex, base, s)?)
            }
            "getprotobyname" => {
                let p = nombres::protocolo_por_nombre(&cadena(ex, a[0], 64)?);
                v(self.poner_protoent(ex, base, p)?)
            }
            "getprotobynumber" => v(self.poner_protoent(ex, base, nombres::protocolo_por_numero(d[0]))?),
            "getnetbyname" => {
                let r = nombres::red_por_nombre(&cadena(ex, a[0], 64)?);
                v(self.poner_netent(ex, base, r)?)
            }
            "getnetbyaddr" => {
                let r = if d[1] == sock::AF_INET { nombres::red_por_numero(d[0]) } else { None };
                v(self.poner_netent(ex, base, r)?)
            }
            "setservent" | "endservent" => {
                self.base_mut(base).recorrido[0] = 0;
                Ok(Hecho::Nada)
            }
            "setprotoent" | "endprotoent" => {
                self.base_mut(base).recorrido[1] = 0;
                Ok(Hecho::Nada)
            }
            "setnetent" | "endnetent" => {
                self.base_mut(base).recorrido[2] = 0;
                Ok(Hecho::Nada)
            }
            "getservent" => {
                let i = self.base_ref(base).recorrido[0];
                self.base_mut(base).recorrido[0] = i + 1;
                v(self.poner_servent(ex, base, nombres::SERVICIOS.get(i))?)
            }
            "getprotoent" => {
                let i = self.base_ref(base).recorrido[1];
                self.base_mut(base).recorrido[1] = i + 1;
                let p = nombres::PROTOCOLOS.get(i).map(|p| (p.0, p.1));
                v(self.poner_protoent(ex, base, p)?)
            }
            "getnetent" => {
                let i = self.base_ref(base).recorrido[2];
                self.base_mut(base).recorrido[2] = i + 1;
                v(self.poner_netent(ex, base, nombres::REDES.get(i).copied())?)
            }
            "gai_strerror" => {
                let c = d[0] as i32;
                let i = if (-13..=0).contains(&c) { (-c) as usize } else { 14 };
                v(self.texto(self.desp_gai[i]))
            }
            "getaddrinfo" => match self.getaddrinfo(ex, red, ll) {
                Err(Mal::Errno(e)) => {
                    self.poner_errno(ex, base, e);
                    v(netdb::EAI_SYSTEM as u32)
                }
                Err(Mal::Fallo(f)) => {
                    self.avisar(ex, "getaddrinfo", f);
                    self.poner_errno(ex, base, EFAULT);
                    v(netdb::EAI_SYSTEM as u32)
                }
                r => r,
            },
            "freeaddrinfo" => {
                self.freeaddrinfo(ex, a[0]);
                Ok(Hecho::Nada)
            }
            "getnameinfo" => v(self.getnameinfo(ex, red, ll)? as u32),
            _ => Err(Mal::NoHecha),
        }
    }

    // --- Resolver ---

    /// Las direcciones de un nombre: una dirección en texto o "localhost" sin preguntar; si no, al servicio, esperando
    /// (la tarea se bloquea y la consulta sigue en el bloqueo).
    fn resolver<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R, ll: &Llamada, n: &[u8]) -> Res<Nombre> {
        let previo = self.reanudar(ex, red, ll);
        if n.is_empty() || n.len() > 255 {
            return Ok(Nombre::Hecho(Err(ErrorRed::SinNombre)));
        }
        if let Some(ip) = nombres::inet_aton(n) {
            return Ok(Nombre::Hecho(Ok(alloc::vec![ip.to_be_bytes()])));
        }
        if n.eq_ignore_ascii_case(b"localhost") {
            return Ok(Nombre::Hecho(Ok(alloc::vec![[127, 0, 0, 1]])));
        }
        if n.eq_ignore_ascii_case(&red.nombre_maquina()) {
            let ip = red.direccion_local();
            return Ok(Nombre::Hecho(Ok(alloc::vec![if ip == [0; 4] { [127, 0, 0, 1] } else { ip }])));
        }
        let mut b = match previo {
            Some(b) => b,
            None => {
                let c = match red.resolver(n) {
                    Ok(c) => c,
                    Err(e) => return Ok(Nombre::Hecho(Err(e))),
                };
                let mut b = Bloqueo::nuevo(ll, Condicion::Nombre, None);
                b.consulta = Some(c);
                b
            }
        };
        if let Some(r) = b.respuesta.take() {
            return Ok(Nombre::Hecho(r));
        }
        if let Some(c) = b.consulta
            && let Some(r) = red.resuelto(c)
        {
            return Ok(Nombre::Hecho(r));
        }
        let c = b.consulta;
        match self.dormir(ex, ll, b, 0) {
            Ok(h) => Ok(Nombre::Espera(h)),
            Err(m) => {
                if let Some(c) = c {
                    red.cerrar(c);
                }
                Err(m)
            }
        }
    }

    /// `gethostbyname(name)` y, con `r = (hp, buf, buflen, he)`, `gethostbyname_r`.
    fn gethostbyname<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        ll: &Llamada,
        r: Option<(u32, u32, u32, u32)>,
    ) -> Res<Hecho> {
        let base = ll.base;
        let n = match cadena(ex, ll.a[0], 1024) {
            Ok(n) => n,
            Err(f) => {
                self.herrno(ex, base, r, netdb::NETDB_INTERNAL);
                return Err(Mal::Fallo(f));
            }
        };
        let dirs = match self.resolver(ex, red, ll, &n) {
            Ok(Nombre::Espera(h)) => return Ok(h),
            Ok(Nombre::Hecho(Ok(d))) => d,
            Ok(Nombre::Hecho(Err(e))) => {
                self.herrno(ex, base, r, e.h_errno());
                return Ok(Hecho::Valor(0));
            }
            Err(m) => {
                self.herrno(ex, base, r, netdb::NETDB_INTERNAL);
                return Err(m);
            }
        };
        self.dar_hostent(ex, base, r, &n, &dirs)
    }

    fn herrno<M: Memoria>(&mut self, ex: &mut Exec<M>, base: u32, r: Option<(u32, u32, u32, u32)>, h: i32) {
        match r {
            Some((_, _, _, he)) if he != 0 => {
                if let Err(f) = e32(ex, he, h as u32) {
                    self.avisar(ex, "gethostbyname_r", f);
                }
            }
            Some(_) => {}
            None => self.poner_herrno(ex, base, h),
        }
    }

    /// La `hostent` con `nombre` y `dirs`: la de la base, o la del programa (`_r`).
    fn dar_hostent<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        base: u32,
        r: Option<(u32, u32, u32, u32)>,
        nombre: &[u8],
        dirs: &[[u8; 4]],
    ) -> Res<Hecho> {
        let dirs = &dirs[..dirs.len().min(privado::MAX_DIRECCIONES as usize)];
        match r {
            None => {
                let z = Zona {
                    hp: base + privado::HOSTENT,
                    alias: base + privado::SIN_ALIAS,
                    lista: base + privado::ADDR_LIST,
                    dirs: base + privado::DIRECCIONES,
                    nombre: base + privado::H_NAME,
                    max_nombre: netdb::MAXHOSTNAMELEN,
                };
                llenar_hostent(ex, &z, nombre, dirs)?;
                self.poner_herrno(ex, base, netdb::NETDB_SUCCESS);
                Ok(Hecho::Valor(z.hp))
            }
            Some((hp, buf, largo, he)) => {
                let n = dirs.len() as u32;
                let total = 4 + 4 * (n + 1) + 4 * n + nombre.len() as u32 + 1;
                if hp == 0 || buf == 0 {
                    return Err(Mal::Fallo(Fallo::PunteroNulo));
                }
                if total > largo {
                    self.herrno(ex, base, r, netdb::NETDB_INTERNAL);
                    return errno(ERANGE);
                }
                tramo(ex, hp, hostent::TAMANO, true)?;
                tramo(ex, buf, total, true)?;
                let z = Zona {
                    hp,
                    alias: buf,
                    lista: buf + 4,
                    dirs: buf + 4 + 4 * (n + 1),
                    nombre: buf + 4 + 8 * n + 4,
                    max_nombre: nombre.len() as u32 + 1,
                };
                llenar_hostent(ex, &z, nombre, dirs)?;
                if he != 0 {
                    e32(ex, he, 0)?;
                }
                Ok(Hecho::Valor(hp))
            }
        }
    }

    /// `gethostbyaddr`: sin búsqueda inversa en DashOS; solo la propia máquina y "localhost".
    #[allow(clippy::too_many_arguments)]
    fn gethostbyaddr<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        base: u32,
        addr: u32,
        largo: u32,
        tipo: u32,
        r: Option<(u32, u32, u32, u32)>,
    ) -> Res<Hecho> {
        if tipo != sock::AF_INET || largo != 4 {
            self.herrno(ex, base, r, netdb::NO_RECOVERY);
            return Ok(Hecho::Valor(0));
        }
        let ip: [u8; 4] = leer_bytes(ex, addr, 4)?.try_into().unwrap();
        let propia = red.direccion_local();
        let nombre = if ip[0] == 127 {
            Vec::from(&b"localhost"[..])
        } else if ip == propia && ip != [0; 4] {
            red.nombre_maquina()
        } else {
            self.herrno(ex, base, r, netdb::HOST_NOT_FOUND);
            return Ok(Hecho::Valor(0));
        };
        self.dar_hostent(ex, base, r, &nombre, &[ip])
    }

    fn poner_servent<M: Memoria>(&mut self, ex: &mut Exec<M>, base: u32, s: Option<&nombres::Servicio>) -> Res<u32> {
        let Some(s) = s else { return Ok(0) };
        let p = base + privado::SERVENT;
        poner_cadena(ex, base + privado::S_NAME, s.nombre.as_bytes(), 32)?;
        poner_cadena(ex, base + privado::S_PROTO, s.proto.as_bytes(), 8)?;
        e32(ex, base + privado::SIN_ALIAS, 0)?;
        e32(ex, p + servent::S_NAME, base + privado::S_NAME)?;
        e32(ex, p + servent::S_ALIASES, base + privado::SIN_ALIAS)?;
        e32(ex, p + servent::S_PORT, s.puerto as u32)?;
        e32(ex, p + servent::S_PROTO, base + privado::S_PROTO)?;
        Ok(p)
    }

    fn poner_protoent<M: Memoria>(&mut self, ex: &mut Exec<M>, base: u32, x: Option<(&str, u32)>) -> Res<u32> {
        let Some((n, num)) = x else { return Ok(0) };
        let p = base + privado::PROTOENT;
        poner_cadena(ex, base + privado::P_NAME, n.as_bytes(), 16)?;
        e32(ex, base + privado::SIN_ALIAS, 0)?;
        e32(ex, p + protoent::P_NAME, base + privado::P_NAME)?;
        e32(ex, p + protoent::P_ALIASES, base + privado::SIN_ALIAS)?;
        e32(ex, p + protoent::P_PROTO, num)?;
        Ok(p)
    }

    fn poner_netent<M: Memoria>(&mut self, ex: &mut Exec<M>, base: u32, x: Option<(&str, u32)>) -> Res<u32> {
        let Some((n, num)) = x else { return Ok(0) };
        let p = base + privado::NETENT;
        poner_cadena(ex, base + privado::N_NAME, n.as_bytes(), 16)?;
        e32(ex, base + privado::SIN_ALIAS, 0)?;
        e32(ex, p + netent::N_NAME, base + privado::N_NAME)?;
        e32(ex, p + netent::N_ALIASES, base + privado::SIN_ALIAS)?;
        e32(ex, p + netent::N_ADDRTYPE, sock::AF_INET)?;
        e32(ex, p + netent::N_NET, num)?;
        Ok(p)
    }

    // --- getaddrinfo ---

    /// `getaddrinfo(host, serv, hints, res)(a0/a1/a2/a3)`: 0 o un `EAI_*`. La lista va en memoria de la tarea (se
    /// suelta con `freeaddrinfo`, o con la tarea).
    fn getaddrinfo<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R, ll: &Llamada) -> Res<Hecho> {
        use netdb::*;
        let (host, serv, hints, res) = (ll.a[0], ll.a[1], ll.a[2], ll.a[3]);
        let ok = |x: i32| Ok(Hecho::Valor(x as u32));
        if res == 0 {
            return Err(Mal::Fallo(Fallo::PunteroNulo));
        }
        tramo(ex, res, 4, true)?;
        let h = if host != 0 { Some(cadena(ex, host, 1024)?) } else { None };
        let s = if serv != 0 { Some(cadena(ex, serv, 256)?) } else { None };
        if h.is_none() && s.is_none() {
            return ok(EAI_NONAME);
        }
        let (flags, familia, tipo, proto) = if hints != 0 {
            tramo(ex, hints, addrinfo::TAMANO, false)?;
            (
                l32(ex, hints + addrinfo::AI_FLAGS)?,
                l32(ex, hints + addrinfo::AI_FAMILY)?,
                l32(ex, hints + addrinfo::AI_SOCKTYPE)?,
                l32(ex, hints + addrinfo::AI_PROTOCOL)?,
            )
        } else {
            (0, 0, 0, 0)
        };
        if flags & !(AI_PASSIVE | AI_CANONNAME | AI_NUMERICHOST | AI_NUMERICSERV) != 0 {
            return ok(EAI_BADFLAGS);
        }
        if familia != sock::AF_UNSPEC && familia != sock::AF_INET {
            return ok(EAI_FAMILY);
        }
        let mut tipos: Vec<(u32, u32, &str)> = Vec::new();
        for (t, p, n) in [(sock::SOCK_STREAM, sock::IPPROTO_TCP, "tcp"), (sock::SOCK_DGRAM, sock::IPPROTO_UDP, "udp")] {
            if (tipo == 0 || tipo == t) && (proto == 0 || proto == p) {
                tipos.push((t, p, n));
            }
        }
        if tipos.is_empty() {
            return ok(if tipo != 0 && tipo != sock::SOCK_STREAM && tipo != sock::SOCK_DGRAM {
                EAI_SOCKTYPE
            } else {
                EAI_SERVICE
            });
        }
        // El puerto: un número, o un nombre de la tabla (solo los tipos en que existe).
        let mut puerto = 0u16;
        if let Some(s) = &s {
            if !s.is_empty() && s.iter().all(u8::is_ascii_digit) && s.len() <= 5 {
                let n: u32 = s.iter().fold(0, |a, &c| a * 10 + (c - b'0') as u32);
                if n > 65535 {
                    return ok(EAI_SERVICE);
                }
                puerto = n as u16;
            } else if flags & AI_NUMERICSERV != 0 {
                return ok(EAI_NONAME);
            } else {
                tipos.retain(|(_, _, n)| nombres::servicio_por_nombre(s, Some(n.as_bytes())).is_some());
                let Some(x) = tipos.first().and_then(|(_, _, n)| nombres::servicio_por_nombre(s, Some(n.as_bytes())))
                else {
                    return ok(EAI_SERVICE);
                };
                puerto = x.puerto;
            }
        }
        let dirs = match &h {
            None => alloc::vec![if flags & AI_PASSIVE != 0 { [0; 4] } else { [127, 0, 0, 1] }],
            Some(n) => match nombres::inet_aton(n) {
                Some(ip) => alloc::vec![ip.to_be_bytes()],
                None if flags & AI_NUMERICHOST != 0 => return ok(EAI_NONAME),
                None => match self.resolver(ex, red, ll, n)? {
                    Nombre::Espera(h) => return Ok(h),
                    Nombre::Hecho(Ok(d)) => d,
                    Nombre::Hecho(Err(e)) => {
                        return ok(match e {
                            ErrorRed::SinNombre => EAI_NONAME,
                            ErrorRed::SinRespuesta | ErrorRed::Limite | ErrorRed::SinDireccion | ErrorRed::Plazo => {
                                EAI_AGAIN
                            }
                            _ => EAI_FAIL,
                        });
                    }
                },
            },
        };
        let canon = if flags & AI_CANONNAME != 0 { h.clone() } else { None };
        let t = ex.tarea_actual();
        let mut nodos: Vec<(u32, u32)> = Vec::new();
        for ip in dirs.iter().take(privado::MAX_DIRECCIONES as usize) {
            for &(tp, pr, _) in &tipos {
                let cn = if nodos.is_empty() { canon.as_ref().map_or(0, |c| c.len() as u32 + 1) } else { 0 };
                let largo = addrinfo::TAMANO + sockaddr_in::TAMANO + cn;
                let d = ex.alloc_mem(largo, memf::PUBLIC | memf::CLEAR)?;
                if d == 0 {
                    for (d, l) in nodos {
                        let _ = ex.free_mem(d, l);
                        self.addrinfos.remove(&d);
                    }
                    return ok(EAI_MEMORY);
                }
                let sa = d + addrinfo::TAMANO;
                e32(ex, d + addrinfo::AI_FLAGS, flags)?;
                e32(ex, d + addrinfo::AI_FAMILY, sock::AF_INET)?;
                e32(ex, d + addrinfo::AI_SOCKTYPE, tp)?;
                e32(ex, d + addrinfo::AI_PROTOCOL, pr)?;
                e32(ex, d + addrinfo::AI_ADDRLEN, sockaddr_in::TAMANO)?;
                e32(ex, d + addrinfo::AI_ADDR, sa)?;
                e8(ex, sa + sockaddr_in::SIN_LEN, sockaddr_in::TAMANO as u8)?;
                e8(ex, sa + sockaddr_in::SIN_FAMILY, sock::AF_INET as u8)?;
                e16(ex, sa + sockaddr_in::SIN_PORT, puerto)?;
                escribir_bytes(ex, sa + sockaddr_in::SIN_ADDR, ip)?;
                if cn > 0 {
                    let c = sa + sockaddr_in::TAMANO;
                    poner_cadena(ex, c, canon.as_ref().unwrap(), cn)?;
                    e32(ex, d + addrinfo::AI_CANONNAME, c)?;
                }
                if let Some(&(antes, _)) = nodos.last() {
                    e32(ex, antes + addrinfo::AI_NEXT, d)?;
                }
                self.addrinfos.insert(d, (largo, t));
                nodos.push((d, largo));
            }
        }
        e32(ex, res, nodos[0].0)?;
        ok(0)
    }

    /// `freeaddrinfo`: suelta la lista. Lo que no dio `getaddrinfo` es un aviso y no se toca.
    fn freeaddrinfo<M: Memoria>(&mut self, ex: &mut Exec<M>, mut d: u32) {
        for _ in 0..4096 {
            if d == 0 {
                return;
            }
            let Some(&(largo, _)) = self.addrinfos.get(&d) else {
                self.avisar(ex, "freeaddrinfo", Fallo::NoReservado { dir: d });
                return;
            };
            let sig = ex.mem.leer32(d + addrinfo::AI_NEXT).unwrap_or(0);
            self.addrinfos.remove(&d);
            if let Err(f) = ex.free_mem(d, largo) {
                self.avisar(ex, "freeaddrinfo", f);
            }
            d = sig;
        }
    }

    /// `getnameinfo(sa, salen, host, hostlen, serv, servlen, flags)(a0/d0/a1/d1/a2/d2/d3)`.
    fn getnameinfo<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R, ll: &Llamada) -> Res<i32> {
        use netdb::*;
        let (sa, salen, host, hostlen, serv, servlen, flags) =
            (ll.a[0], ll.d[0], ll.a[1], ll.d[1], ll.a[2], ll.d[2], ll.d[3]);
        let (familia, ip, puerto) = crate::zocalos::leer_sockaddr(ex, sa, salen)?;
        if familia != sock::AF_INET {
            return Ok(EAI_FAMILY);
        }
        if host != 0 && hostlen > 0 {
            let propia = red.direccion_local();
            let nombre = if flags & NI_NUMERICHOST != 0 {
                None
            } else if ip[0] == 127 {
                Some(Vec::from(&b"localhost"[..]))
            } else if ip == propia && ip != [0; 4] {
                Some(red.nombre_maquina())
            } else {
                None
            };
            if nombre.is_none() && flags & NI_NAMEREQD != 0 {
                return Ok(EAI_NONAME);
            }
            let t = nombre.unwrap_or_else(|| nombres::ntoa(u32::from_be_bytes(ip)));
            if t.len() as u32 + 1 > hostlen {
                return Ok(EAI_FAIL);
            }
            tramo(ex, host, t.len() as u32 + 1, true)?;
            poner_cadena(ex, host, &t, hostlen)?;
        }
        if serv != 0 && servlen > 0 {
            let proto: &[u8] = if flags & NI_DGRAM != 0 { b"udp" } else { b"tcp" };
            let t = match nombres::servicio_por_puerto(puerto, Some(proto)) {
                Some(s) if flags & NI_NUMERICSERV == 0 => Vec::from(s.nombre.as_bytes()),
                _ => Vec::from(format!("{}", puerto).as_bytes()),
            };
            if t.len() as u32 + 1 > servlen {
                return Ok(EAI_FAIL);
            }
            tramo(ex, serv, t.len() as u32 + 1, true)?;
            poner_cadena(ex, serv, &t, servlen)?;
        }
        Ok(0)
    }
}
