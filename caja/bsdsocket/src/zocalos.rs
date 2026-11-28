//! Los zócalos: `socket`, `bind`, `listen`, `accept`, `connect`, `send*`, `recv*`, `shutdown`, `setsockopt`,
//! `getsockopt`, `getsockname`, `getpeername`, `IoctlSocket`, `CloseSocket`, `WaitSelect`, `Dup2Socket`,
//! `ObtainSocket`, `ReleaseSocket`, `ReleaseCopyOfSocket` y `GetSocketEvents`, de sus autodocs.

use alloc::vec::Vec;

use caja_exec::{Exec, Fallo, Memoria};

use crate::definiciones::errno::*;
use crate::definiciones::{fio, iovec, linger, msghdr, sbt, sock, sockaddr_in, timeval};
use crate::red::{Fase, Red};
use crate::*;

/// De dónde salen los datos de un envío: la memoria del 68k o lo ya juntado (`sendmsg`).
pub(crate) enum Fuente<'a> {
    Memoria(u32, u32),
    Bytes(&'a [u8]),
}

impl Fuente<'_> {
    fn largo(&self) -> u32 {
        match self {
            Fuente::Memoria(_, l) => *l,
            Fuente::Bytes(b) => b.len() as u32,
        }
    }
    fn comprobar<M: Memoria>(&self, ex: &Exec<M>) -> Result<(), Fallo> {
        match self {
            Fuente::Memoria(d, l) => tramo(ex, *d, *l, false),
            Fuente::Bytes(_) => Ok(()),
        }
    }
    /// `n` bytes desde `desde`.
    fn trozo<M: Memoria>(&self, ex: &Exec<M>, desde: u32, n: u32) -> Result<Vec<u8>, Fallo> {
        match self {
            Fuente::Memoria(d, _) => leer_bytes(ex, d + desde, n),
            Fuente::Bytes(b) => Ok(b[desde as usize..(desde + n) as usize].to_vec()),
        }
    }
}

/// A dónde van los datos recibidos: un búfer del 68k o varios (`recvmsg`).
pub(crate) enum Destino {
    Memoria(u32, u32),
    Iov(Vec<(u32, u32)>),
}

impl Destino {
    fn largo(&self) -> u32 {
        match self {
            Destino::Memoria(_, l) => *l,
            Destino::Iov(v) => v.iter().map(|x| x.1).fold(0u32, u32::saturating_add),
        }
    }
    fn comprobar<M: Memoria>(&self, ex: &Exec<M>) -> Result<(), Fallo> {
        match self {
            Destino::Memoria(d, l) => tramo(ex, *d, *l, true),
            Destino::Iov(v) => v.iter().try_for_each(|&(d, l)| tramo(ex, d, l, true)),
        }
    }
    /// Escribe `datos` desde el byte `desde` del destino.
    fn escribir<M: Memoria>(&self, ex: &mut Exec<M>, desde: u32, datos: &[u8]) -> Result<(), Fallo> {
        match self {
            Destino::Memoria(d, _) => escribir_bytes(ex, d + desde, datos),
            Destino::Iov(v) => {
                let (mut pos, mut resto) = (0u32, datos);
                for &(d, l) in v {
                    if resto.is_empty() {
                        break;
                    }
                    let fin = pos.saturating_add(l);
                    if fin > desde {
                        let dentro = desde.saturating_sub(pos);
                        let n = ((l - dentro) as usize).min(resto.len());
                        escribir_bytes(ex, d + dentro, &resto[..n])?;
                        resto = &resto[n..];
                    }
                    pos = fin;
                }
                Ok(())
            }
        }
    }
}

/// Lee una `sockaddr_in`: familia, dirección y puerto.
pub(crate) fn leer_sockaddr<M: Memoria>(ex: &Exec<M>, dir: u32, largo: u32) -> Res<(u32, [u8; 4], u16)> {
    if dir == 0 {
        return Err(Mal::Fallo(Fallo::PunteroNulo));
    }
    if (largo as i32) < 8 {
        return errno(EINVAL);
    }
    let b = leer_bytes(ex, dir, 8)?;
    Ok((b[1] as u32, [b[4], b[5], b[6], b[7]], u16::from_be_bytes([b[2], b[3]])))
}

/// Escribe una `sockaddr_in` en `dir`, lo que quepa en `*lenp`, y deja en `*lenp` su tamaño. Con `dir` 0, nada.
pub(crate) fn poner_sockaddr<M: Memoria>(ex: &mut Exec<M>, dir: u32, lenp: u32, ip: [u8; 4], puerto: u16) -> Res<()> {
    if dir == 0 {
        return Ok(());
    }
    if lenp == 0 {
        return Err(Mal::Fallo(Fallo::PunteroNulo));
    }
    let l = l32(ex, lenp)? as i32;
    if l < 0 {
        return errno(EINVAL);
    }
    let p = puerto.to_be_bytes();
    let img = [
        sockaddr_in::TAMANO as u8,
        sock::AF_INET as u8,
        p[0],
        p[1],
        ip[0],
        ip[1],
        ip[2],
        ip[3],
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    let n = (l as usize).min(img.len());
    tramo(ex, dir, n as u32, true)?;
    escribir_bytes(ex, dir, &img[..n])?;
    e32(ex, lenp, sockaddr_in::TAMANO)?;
    Ok(())
}

fn es_local(ip: [u8; 4], propia: [u8; 4]) -> bool {
    ip == [0; 4] || ip[0] == 127 || ip == propia
}

fn microsegundos<M: Memoria>(ex: &Exec<M>, d: u32) -> Res<u64> {
    let s = l32(ex, d + timeval::TV_SECS)? as u64;
    let us = l32(ex, d + timeval::TV_MICRO)? as u64;
    Ok(s * 1_000_000 + us.min(999_999))
}

impl Bsdsocket {
    fn z(&self, z: u32) -> &Zocalo {
        &self.zocalos[&z]
    }
    fn zm(&mut self, z: u32) -> &mut Zocalo {
        self.zocalos.get_mut(&z).expect("zócalo")
    }

    /// Pone un zócalo que ya existe en el primer descriptor libre (con una referencia más si `otra`).
    fn ocupar(&mut self, base: u32, z: u32, otra: bool) -> Res<u32> {
        let b = self.base_mut(base);
        let Some(fd) = b.fds.iter().position(Option::is_none) else { return errno(EMFILE) };
        b.fds[fd] = Some(z);
        if otra {
            self.zm(z).refs += 1;
        }
        Ok(fd as u32)
    }

    /// Cuánto falta para que venza una espera: el plazo de la primera llamada o el de la opción.
    fn plazo(previo: &Option<Bloqueo>, ahora: u64, opcion_us: u64) -> Option<u64> {
        match previo {
            Some(b) => b.vence,
            None if opcion_us > 0 => Some(ahora.saturating_add(opcion_us)),
            None => None,
        }
    }

    // --- Mirar cómo está ---

    /// Avanza lo que va solo (un `connect` que acabó, una entrada en un oído) y dice si se puede leer, escribir o hay
    /// algo excepcional. Un zócalo que ya no existe está "listo" para todo (quien lo espere verá `EBADF`).
    pub(crate) fn mirar<R: Red>(&mut self, red: &mut R, z: u32) -> u8 {
        let Some(zz) = self.zocalos.get_mut(&z) else { return LEER | ESCRIBIR | EXCEPCION };
        match &mut zz.estado {
            Estado::Conectando(c) => {
                let c = *c;
                match red.fase(c) {
                    Fase::Conectando => {}
                    Fase::Abierta | Fase::Fin => {
                        zz.estado = Estado::Conectado(c);
                        zz.eventos |= sbt::FD_CONNECT;
                    }
                    Fase::Error(e) => {
                        red.cerrar(c);
                        zz.error = e.errno(true);
                        zz.estado = Estado::Fallido;
                        zz.eventos |= sbt::FD_CONNECT | sbt::FD_ERROR;
                    }
                }
            }
            Estado::Escuchando { puerto, cola, oidos, entradas } => {
                let mut i = 0;
                while i < oidos.len() && entradas.len() < *cola {
                    match red.aceptar(oidos[i]) {
                        Ok(None) => i += 1,
                        r => {
                            match r {
                                Ok(Some(c)) => entradas.push_back(c),
                                _ => red.cerrar(oidos[i]),
                            }
                            // Ese oído ya no sirve: otro en su lugar.
                            match red.escuchar(*puerto) {
                                Ok(n) => {
                                    oidos[i] = n;
                                    i += 1;
                                }
                                Err(_) => {
                                    oidos.remove(i);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        match &zz.estado {
            Estado::Nuevo => {
                if zz.tipo == Tipo::Datagrama {
                    ESCRIBIR
                } else {
                    0
                }
            }
            Estado::Conectando(_) => 0,
            Estado::Conectado(c) => {
                let c = *c;
                let f = red.fase(c);
                let (rec, sitio) = red.pendiente(c);
                let mut b = 0;
                if !zz.guardado.is_empty() || rec > 0 || zz.sin_lectura || f != Fase::Abierta {
                    b |= LEER;
                }
                if f != Fase::Abierta || sitio > 0 {
                    b |= ESCRIBIR;
                }
                b
            }
            Estado::Escuchando { entradas, .. } => {
                if entradas.is_empty() {
                    0
                } else {
                    LEER
                }
            }
            Estado::Datagramas(u) => {
                let u = *u;
                let roto = matches!(red.fase(u), Fase::Error(_));
                let (rec, sitio) = red.pendiente(u);
                let mut b = 0;
                if zz.datagrama.is_some() || rec > 0 || zz.sin_lectura || roto {
                    b |= LEER;
                }
                if sitio > 0 || roto {
                    b |= ESCRIBIR;
                }
                b
            }
            Estado::Fallido => LEER | ESCRIBIR,
        }
    }

    /// El sitio para enviar de un zócalo con conexión o UDP.
    pub(crate) fn sitio<R: Red>(&mut self, red: &mut R, z: u32) -> Option<usize> {
        match self.zocalos.get(&z).map(|x| &x.estado) {
            Some(Estado::Conectado(c)) | Some(Estado::Datagramas(c)) => Some(red.pendiente(*c).1),
            _ => None,
        }
    }

    /// El zócalo UDP, abierto (en el puerto de `bind` si lo hubo).
    fn abrir_udp<R: Red>(&mut self, red: &mut R, z: u32) -> Res<Id> {
        let zz = self.zm(z);
        match zz.estado {
            Estado::Datagramas(u) => Ok(u),
            Estado::Nuevo => match red.udp_abrir(zz.local.1) {
                Ok(u) => {
                    zz.estado = Estado::Datagramas(u);
                    Ok(u)
                }
                Err(e) => errno(e.errno(false)),
            },
            _ => errno(EINVAL),
        }
    }

    // --- Crear, atar, escuchar ---

    pub(crate) fn socket(&mut self, base: u32, dominio: u32, tipo: u32, protocolo: u32) -> Res<u32> {
        if dominio != sock::AF_INET {
            return errno(EAFNOSUPPORT);
        }
        let (t, p) = match tipo {
            sock::SOCK_STREAM if protocolo == 0 || protocolo == sock::IPPROTO_TCP => (Tipo::Flujo, sock::IPPROTO_TCP),
            sock::SOCK_DGRAM if protocolo == 0 || protocolo == sock::IPPROTO_UDP => {
                (Tipo::Datagrama, sock::IPPROTO_UDP)
            }
            sock::SOCK_STREAM | sock::SOCK_DGRAM => return errno(EPROTONOSUPPORT),
            _ => return errno(ESOCKTNOSUPPORT),
        };
        self.nuevo_zocalo(base, Zocalo::nuevo(t, p))
    }

    pub(crate) fn bind<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        base: u32,
        fd: u32,
        nombre: u32,
        largo: u32,
    ) -> Res<u32> {
        let z = self.zocalo_de(base, fd)?;
        let (familia, ip, puerto) = leer_sockaddr(ex, nombre, largo)?;
        if familia != sock::AF_INET && familia != sock::AF_UNSPEC {
            return errno(EAFNOSUPPORT);
        }
        let zz = self.z(z);
        if zz.atado || !matches!(zz.estado, Estado::Nuevo) {
            return errno(EINVAL);
        }
        if !es_local(ip, red.direccion_local()) {
            return errno(EADDRNOTAVAIL);
        }
        let reusa = |x: &Zocalo| x.opciones.get(&(sock::SOL_SOCKET, sock::SO_REUSEADDR)).is_some_and(|&v| v != 0);
        if zz.tipo == Tipo::Flujo {
            if puerto != 0
                && !reusa(zz)
                && self
                    .zocalos
                    .iter()
                    .any(|(&k, x)| k != z && x.tipo == Tipo::Flujo && x.atado && x.local.1 == puerto && !reusa(x))
            {
                return errno(EADDRINUSE);
            }
        } else {
            match red.udp_abrir(puerto) {
                Ok(u) => self.zm(z).estado = Estado::Datagramas(u),
                Err(e) => return errno(e.errno(false)),
            }
        }
        let zz = self.zm(z);
        zz.local = (ip, puerto);
        zz.atado = true;
        Ok(0)
    }

    pub(crate) fn listen<R: Red>(&mut self, red: &mut R, base: u32, fd: u32, cola: u32) -> Res<u32> {
        let z = self.zocalo_de(base, fd)?;
        let cola = (cola as i32).clamp(1, 8) as usize;
        let zz = self.zm(z);
        if zz.tipo != Tipo::Flujo {
            return errno(EOPNOTSUPP);
        }
        match &mut zz.estado {
            Estado::Escuchando { puerto, cola: c, oidos, .. } => {
                *c = cola;
                while oidos.len() < cola {
                    match red.escuchar(*puerto) {
                        Ok(o) => oidos.push(o),
                        Err(_) => break,
                    }
                }
                return Ok(0);
            }
            Estado::Nuevo => {}
            _ => return errno(EINVAL),
        }
        // El primer oído dice si el puerto vale; sin puerto (bind al 0, como el modo activo de FTP), uno libre.
        let (puerto, primero) = if zz.local.1 != 0 {
            match red.escuchar(zz.local.1) {
                Ok(o) => (zz.local.1, o),
                Err(e) => return errno(e.errno(false)),
            }
        } else {
            let mut hallado = None;
            for _ in 0..32 {
                let p = 49152 + self.efimero % 16384;
                self.efimero = self.efimero.wrapping_add(1);
                match red.escuchar(p) {
                    Ok(o) => {
                        hallado = Some((p, o));
                        break;
                    }
                    Err(crate::red::ErrorRed::Ocupado) => continue,
                    Err(e) => return errno(e.errno(false)),
                }
            }
            match hallado {
                Some(h) => h,
                None => return errno(EADDRINUSE),
            }
        };
        let mut oidos = alloc::vec![primero];
        while oidos.len() < cola {
            match red.escuchar(puerto) {
                Ok(o) => oidos.push(o),
                Err(_) => break,
            }
        }
        let zz = self.zm(z);
        zz.estado = Estado::Escuchando { puerto, cola, oidos, entradas: Default::default() };
        zz.local.1 = puerto;
        zz.atado = true;
        Ok(0)
    }

    pub(crate) fn accept<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        ll: &Llamada,
        fd: u32,
        addr: u32,
        lenp: u32,
    ) -> Res<Hecho> {
        let previo = self.reanudar(ex, red, ll);
        let z = self.zocalo_de(ll.base, fd)?;
        if addr != 0 {
            if lenp == 0 {
                return Err(Mal::Fallo(Fallo::PunteroNulo));
            }
            let l = (l32(ex, lenp)? as i32).clamp(0, sockaddr_in::TAMANO as i32) as u32;
            tramo(ex, addr, l, true)?;
        }
        if !matches!(self.z(z).estado, Estado::Escuchando { .. }) {
            return errno(EINVAL);
        }
        self.mirar(red, z);
        let hay = matches!(&self.z(z).estado, Estado::Escuchando { entradas, .. } if !entradas.is_empty());
        if hay {
            if !self.base_ref(ll.base).fds.iter().any(Option::is_none) {
                return errno(EMFILE);
            }
            let Estado::Escuchando { entradas, .. } = &mut self.zm(z).estado else { unreachable!() };
            let c = entradas.pop_front().unwrap();
            let ext = red.extremos(c).unwrap_or_default();
            let ip_local = self.z(z).local.0;
            let mut nuevo = Zocalo::nuevo(Tipo::Flujo, sock::IPPROTO_TCP);
            nuevo.estado = Estado::Conectado(c);
            nuevo.local = (ip_local, ext.local_puerto);
            nuevo.atado = true;
            nuevo.remoto = Some((ext.remoto, ext.remoto_puerto));
            let nfd = self.nuevo_zocalo(ll.base, nuevo)?;
            poner_sockaddr(ex, addr, lenp, ext.remoto, ext.remoto_puerto)?;
            return Ok(Hecho::Valor(nfd));
        }
        if self.z(z).no_bloqueante {
            return errno(EWOULDBLOCK);
        }
        let vence = Self::plazo(&previo, ll.ahora, self.z(z).rcvtimeo_us);
        if vence.is_some_and(|v| v <= ll.ahora) {
            return errno(EWOULDBLOCK);
        }
        self.dormir(ex, ll, Bloqueo::nuevo(ll, Condicion::Leer(z), vence), 0)
    }

    pub(crate) fn connect<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        ll: &Llamada,
        fd: u32,
        nombre: u32,
        largo: u32,
    ) -> Res<Hecho> {
        let previo = self.reanudar(ex, red, ll);
        let z = self.zocalo_de(ll.base, fd)?;
        if self.z(z).tipo == Tipo::Datagrama {
            let (familia, ip, puerto) = leer_sockaddr(ex, nombre, largo)?;
            if familia == sock::AF_UNSPEC {
                self.zm(z).remoto = None;
                return Ok(Hecho::Valor(0));
            }
            if familia != sock::AF_INET {
                return errno(EAFNOSUPPORT);
            }
            self.abrir_udp(red, z)?;
            let ip = if ip == [0; 4] { [127, 0, 0, 1] } else { ip };
            self.zm(z).remoto = Some((ip, puerto));
            return Ok(Hecho::Valor(0));
        }
        let nb = self.z(z).no_bloqueante;
        match self.z(z).estado {
            Estado::Conectado(_) => return if previo.is_some() { Ok(Hecho::Valor(0)) } else { errno(EISCONN) },
            Estado::Conectando(_) if previo.is_none() && nb => return errno(EALREADY),
            Estado::Conectando(_) => {}
            Estado::Escuchando { .. } => return errno(EINVAL),
            Estado::Datagramas(_) => return errno(EINVAL),
            Estado::Fallido if self.z(z).error != 0 => {
                let e = core::mem::take(&mut self.zm(z).error);
                return errno(e);
            }
            Estado::Nuevo | Estado::Fallido => {
                let (familia, ip, puerto) = leer_sockaddr(ex, nombre, largo)?;
                if familia != sock::AF_INET {
                    return errno(EAFNOSUPPORT);
                }
                if puerto == 0 {
                    return errno(EADDRNOTAVAIL);
                }
                let ip = if ip == [0; 4] { [127, 0, 0, 1] } else { ip };
                match red.conectar(ip, puerto) {
                    Ok(c) => {
                        let zz = self.zm(z);
                        zz.estado = Estado::Conectando(c);
                        zz.remoto = Some((ip, puerto));
                    }
                    Err(e) => return errno(e.errno(true)),
                }
            }
        }
        self.mirar(red, z);
        match self.z(z).estado {
            Estado::Conectado(_) => Ok(Hecho::Valor(0)),
            Estado::Fallido => {
                let e = core::mem::take(&mut self.zm(z).error);
                errno(if e == 0 { ECONNREFUSED } else { e })
            }
            _ if nb => errno(if previo.is_some() { EALREADY } else { EINPROGRESS }),
            _ => {
                let vence = previo.as_ref().and_then(|b| b.vence);
                self.dormir(ex, ll, Bloqueo::nuevo(ll, Condicion::Conectar(z), vence), 0)
            }
        }
    }

    // --- Enviar y recibir ---

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn enviar<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        ll: &Llamada,
        fd: u32,
        fuente: Fuente,
        flags: u32,
        destino: Option<(u32, u32)>,
    ) -> Res<Hecho> {
        let previo = self.reanudar(ex, red, ll);
        let z = self.zocalo_de(ll.base, fd)?;
        let largo = fuente.largo();
        fuente.comprobar(ex)?;
        if flags & sock::MSG_OOB != 0 {
            return errno(EOPNOTSUPP);
        }
        let nb = self.z(z).no_bloqueante || flags & sock::MSG_DONTWAIT != 0;
        let vence = Self::plazo(&previo, ll.ahora, self.z(z).sndtimeo_us);
        let vencido = vence.is_some_and(|v| v <= ll.ahora);
        if self.z(z).sin_escritura {
            return errno(EPIPE);
        }
        if self.z(z).tipo == Tipo::Datagrama {
            let (ip, puerto) = match destino {
                Some((p, l)) if p != 0 => {
                    let (familia, ip, puerto) = leer_sockaddr(ex, p, l)?;
                    if familia != sock::AF_INET && familia != sock::AF_UNSPEC {
                        return errno(EAFNOSUPPORT);
                    }
                    (if ip == [0; 4] { [127, 0, 0, 1] } else { ip }, puerto)
                }
                _ => match self.z(z).remoto {
                    Some(r) => r,
                    None => return errno(EDESTADDRREQ),
                },
            };
            if largo as usize > red.max_datagrama() {
                return errno(EMSGSIZE);
            }
            let u = self.abrir_udp(red, z)?;
            let datos = fuente.trozo(ex, 0, largo)?;
            if red.udp_enviar(u, &datos, ip, puerto) {
                self.enviados += largo as u64;
                return Ok(Hecho::Valor(largo));
            }
            if let Fase::Error(e) = red.fase(u) {
                return errno(e.errno(false));
            }
            if nb || vencido {
                return errno(EWOULDBLOCK);
            }
            let cond = Condicion::Escribir(z, largo as usize + 24);
            return self.dormir(ex, ll, Bloqueo::nuevo(ll, cond, vence), 0);
        }
        let mut hecho = previo.as_ref().map_or(0, |b| b.hecho).min(largo);
        self.mirar(red, z);
        let c = match self.z(z).estado {
            Estado::Conectado(c) => Some(c),
            Estado::Conectando(_) => None,
            Estado::Fallido => {
                let e = core::mem::take(&mut self.zm(z).error);
                return errno(if e == 0 { EPIPE } else { e });
            }
            _ => return errno(ENOTCONN),
        };
        if let Some(c) = c {
            if largo == 0 {
                return Ok(Hecho::Valor(0));
            }
            match red.fase(c) {
                Fase::Fin => return if hecho > 0 { Ok(Hecho::Valor(hecho)) } else { errno(EPIPE) },
                Fase::Error(e) => return if hecho > 0 { Ok(Hecho::Valor(hecho)) } else { errno(e.errno(false)) },
                Fase::Conectando => {}
                Fase::Abierta => {
                    while hecho < largo {
                        let sitio = red.pendiente(c).1 as u32;
                        if sitio == 0 {
                            break;
                        }
                        let n = sitio.min(largo - hecho).min(1 << 20);
                        let datos = fuente.trozo(ex, hecho, n)?;
                        let m = red.enviar(c, &datos) as u32;
                        hecho += m.min(n);
                        self.enviados += m as u64;
                        if m == 0 {
                            break;
                        }
                    }
                }
            }
            if hecho == largo {
                return Ok(Hecho::Valor(largo));
            }
        }
        if nb || vencido {
            return if hecho > 0 { Ok(Hecho::Valor(hecho)) } else { errno(EWOULDBLOCK) };
        }
        let mut b = Bloqueo::nuevo(ll, Condicion::Escribir(z, 1), vence);
        b.hecho = hecho;
        match self.dormir(ex, ll, b, 0) {
            Err(Mal::Errno(EINTR)) if hecho > 0 => Ok(Hecho::Valor(hecho)),
            x => x,
        }
    }

    /// Lo común de `recv`, `recvfrom` y `recvmsg`. Con el resultado, si el datagrama no cupo entero.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn recibir<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        ll: &Llamada,
        fd: u32,
        destino: Destino,
        flags: u32,
        origen: Option<(u32, u32)>,
    ) -> Res<(Hecho, bool)> {
        let previo = self.reanudar(ex, red, ll);
        let z = self.zocalo_de(ll.base, fd)?;
        let largo = destino.largo();
        destino.comprobar(ex)?;
        if let Some((a, lp)) = origen
            && a != 0
            && lp == 0
        {
            return Err(Mal::Fallo(Fallo::PunteroNulo));
        }
        if flags & sock::MSG_OOB != 0 {
            return errno(EINVAL);
        }
        let nb = self.z(z).no_bloqueante || flags & sock::MSG_DONTWAIT != 0;
        let mirar = flags & sock::MSG_PEEK != 0;
        let todo = flags & sock::MSG_WAITALL != 0 && !mirar;
        let vence = Self::plazo(&previo, ll.ahora, self.z(z).rcvtimeo_us);
        let vencido = vence.is_some_and(|v| v <= ll.ahora);
        let (oa, olp) = origen.unwrap_or((0, 0));
        if self.z(z).tipo == Tipo::Datagrama {
            let u = self.abrir_udp(red, z)?;
            for _ in 0..256 {
                let d = match self.zm(z).datagrama.take() {
                    Some(d) => Some(d),
                    None => {
                        self.tmp.resize(MAX_DATAGRAMA, 0);
                        red.udp_recibir(u, &mut self.tmp)
                            .map(|(n, ip, p)| (self.tmp[..n.min(MAX_DATAGRAMA)].to_vec(), ip, p))
                    }
                };
                let Some((datos, ip, puerto)) = d else { break };
                if let Some(r) = self.z(z).remoto
                    && r != (ip, puerto)
                {
                    // Conectado: lo de otros se tira.
                    continue;
                }
                let n = (largo as usize).min(datos.len());
                destino.escribir(ex, 0, &datos[..n])?;
                let truncado = datos.len() > largo as usize;
                poner_sockaddr(ex, oa, olp, ip, puerto)?;
                self.recibidos += n as u64;
                if mirar {
                    self.zm(z).datagrama = Some((datos, ip, puerto));
                }
                return Ok((Hecho::Valor(n as u32), truncado));
            }
            if self.z(z).sin_lectura {
                return Ok((Hecho::Valor(0), false));
            }
            if let Fase::Error(e) = red.fase(u) {
                return errno(e.errno(false));
            }
            if nb || vencido {
                return errno(EWOULDBLOCK);
            }
            return self.dormir(ex, ll, Bloqueo::nuevo(ll, Condicion::Leer(z), vence), 0).map(|h| (h, false));
        }
        let mut hecho = previo.as_ref().map_or(0, |b| b.hecho).min(largo);
        self.mirar(red, z);
        let c = match self.z(z).estado {
            Estado::Conectado(c) => Some(c),
            Estado::Conectando(_) => None,
            Estado::Fallido => {
                let e = core::mem::take(&mut self.zm(z).error);
                return errno(if e == 0 { ENOTCONN } else { e });
            }
            _ => return errno(ENOTCONN),
        };
        if let Some(c) = c {
            if self.z(z).sin_lectura || largo == 0 {
                return Ok((Hecho::Valor(hecho), false));
            }
            if mirar {
                // Lo que haya, sin sacarlo: se guarda aquí hasta que se lea.
                let falta = (largo as usize).saturating_sub(self.z(z).guardado.len()).min(1 << 16);
                if falta > 0 {
                    self.tmp.resize(falta, 0);
                    let m = red.recibir(c, &mut self.tmp[..falta]);
                    let zz = self.zocalos.get_mut(&z).unwrap();
                    zz.guardado.extend_from_slice(&self.tmp[..m]);
                }
                let g = &self.z(z).guardado;
                let n = g.len().min(largo as usize);
                if n > 0 {
                    let copia = g[..n].to_vec();
                    destino.escribir(ex, 0, &copia)?;
                    if let Some((ip, p)) = self.z(z).remoto {
                        poner_sockaddr(ex, oa, olp, ip, p)?;
                    }
                    return Ok((Hecho::Valor(n as u32), false));
                }
            } else {
                while hecho < largo {
                    let falta = (largo - hecho) as usize;
                    let g = core::mem::take(&mut self.zm(z).guardado);
                    if !g.is_empty() {
                        let n = g.len().min(falta);
                        destino.escribir(ex, hecho, &g[..n])?;
                        self.zm(z).guardado = g[n..].to_vec();
                        hecho += n as u32;
                        continue;
                    }
                    let k = falta.min(1 << 16);
                    self.tmp.resize(k, 0);
                    let m = red.recibir(c, &mut self.tmp[..k]);
                    if m == 0 {
                        break;
                    }
                    let datos = self.tmp[..m].to_vec();
                    destino.escribir(ex, hecho, &datos)?;
                    hecho += m as u32;
                    self.recibidos += m as u64;
                    if !todo {
                        break;
                    }
                }
                if hecho > 0
                    && let Some((ip, p)) = self.z(z).remoto
                {
                    poner_sockaddr(ex, oa, olp, ip, p)?;
                }
                if hecho == largo || (hecho > 0 && !todo) {
                    return Ok((Hecho::Valor(hecho), false));
                }
            }
            match red.fase(c) {
                Fase::Fin => return Ok((Hecho::Valor(hecho), false)),
                Fase::Error(e) => {
                    return if hecho > 0 { Ok((Hecho::Valor(hecho), false)) } else { errno(e.errno(false)) };
                }
                _ => {}
            }
        }
        if nb || vencido {
            return if hecho > 0 { Ok((Hecho::Valor(hecho), false)) } else { errno(EWOULDBLOCK) };
        }
        let mut b = Bloqueo::nuevo(ll, Condicion::Leer(z), vence);
        b.hecho = hecho;
        match self.dormir(ex, ll, b, 0) {
            Err(Mal::Errno(EINTR)) if hecho > 0 => Ok((Hecho::Valor(hecho), false)),
            x => x.map(|h| (h, false)),
        }
    }

    pub(crate) fn sendmsg<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R, ll: &Llamada) -> Res<Hecho> {
        let (fd, msg, flags) = (ll.d[0], ll.a[0], ll.d[1]);
        if msg == 0 {
            return Err(Mal::Fallo(Fallo::PunteroNulo));
        }
        tramo(ex, msg, msghdr::TAMANO, false)?;
        let nombre = l32(ex, msg + msghdr::MSG_NAME)?;
        let largo_nombre = l32(ex, msg + msghdr::MSG_NAMELEN)?;
        let iov = self.iovecs(ex, msg)?;
        let mut datos = Vec::new();
        for (d, l) in iov {
            if datos.len() + l as usize > 1 << 20 {
                return errno(EMSGSIZE);
            }
            datos.extend_from_slice(&leer_bytes(ex, d, l)?);
        }
        let destino = if nombre != 0 { Some((nombre, largo_nombre)) } else { None };
        self.enviar(ex, red, ll, fd, Fuente::Bytes(&datos), flags, destino)
    }

    pub(crate) fn recvmsg<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R, ll: &Llamada) -> Res<Hecho> {
        let (fd, msg, flags) = (ll.d[0], ll.a[0], ll.d[1]);
        if msg == 0 {
            return Err(Mal::Fallo(Fallo::PunteroNulo));
        }
        tramo(ex, msg, msghdr::TAMANO, true)?;
        let nombre = l32(ex, msg + msghdr::MSG_NAME)?;
        let iov = self.iovecs(ex, msg)?;
        let origen = if nombre != 0 { Some((nombre, msg + msghdr::MSG_NAMELEN)) } else { None };
        let (h, truncado) = self.recibir(ex, red, ll, fd, Destino::Iov(iov), flags, origen)?;
        if let Hecho::Valor(_) = h {
            e32(ex, msg + msghdr::MSG_FLAGS, if truncado { sock::MSG_TRUNC } else { 0 })?;
            e32(ex, msg + msghdr::MSG_CONTROLLEN, 0)?;
        }
        Ok(h)
    }

    fn iovecs<M: Memoria>(&self, ex: &Exec<M>, msg: u32) -> Res<Vec<(u32, u32)>> {
        let v = l32(ex, msg + msghdr::MSG_IOV)?;
        let n = l32(ex, msg + msghdr::MSG_IOVLEN)?;
        if n > 1024 {
            return errno(EMSGSIZE);
        }
        if n > 0 && v == 0 {
            return Err(Mal::Fallo(Fallo::PunteroNulo));
        }
        tramo(ex, v, n * iovec::TAMANO, false)?;
        (0..n)
            .map(|i| {
                let d = v + i * iovec::TAMANO;
                Ok((l32(ex, d + iovec::IOV_BASE)?, l32(ex, d + iovec::IOV_LEN)?))
            })
            .collect()
    }

    pub(crate) fn shutdown<R: Red>(&mut self, _red: &mut R, base: u32, fd: u32, como: u32) -> Res<u32> {
        let z = self.zocalo_de(base, fd)?;
        if como > sock::SHUT_RDWR {
            return errno(EINVAL);
        }
        let zz = self.zm(z);
        let conectado = match zz.estado {
            Estado::Conectado(_) => true,
            Estado::Datagramas(_) => zz.remoto.is_some(),
            _ => false,
        };
        if !conectado {
            return errno(ENOTCONN);
        }
        // DashOS no cierra media conexión: SHUT_WR deja de enviar aquí (la conexión se cierra con CloseSocket).
        if como != sock::SHUT_WR {
            zz.sin_lectura = true;
        }
        if como != sock::SHUT_RD {
            zz.sin_escritura = true;
        }
        Ok(0)
    }

    // --- Opciones ---

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn setsockopt<M: Memoria>(
        &mut self,
        ex: &mut Exec<M>,
        base: u32,
        fd: u32,
        nivel: u32,
        nombre: u32,
        valor: u32,
        largo: u32,
    ) -> Res<u32> {
        use sock::*;
        let z = self.zocalo_de(base, fd)?;
        if valor == 0 && largo > 0 {
            return Err(Mal::Fallo(Fallo::PunteroNulo));
        }
        let v = match largo {
            0 => 0,
            1 => l8(ex, valor)? as u32,
            2 | 3 => l16(ex, valor)? as u32,
            _ => l32(ex, valor)?,
        };
        let tipo = self.z(z).tipo;
        match (nivel, nombre) {
            (SOL_SOCKET, SO_LINGER) => {
                if largo < linger::TAMANO {
                    return errno(EINVAL);
                }
                let l = (l32(ex, valor + linger::L_ONOFF)?, l32(ex, valor + linger::L_LINGER)?);
                self.zm(z).linger = l;
            }
            (SOL_SOCKET, SO_RCVTIMEO) | (SOL_SOCKET, SO_SNDTIMEO) => {
                if largo < timeval::TAMANO {
                    return errno(EINVAL);
                }
                let us = microsegundos(ex, valor)?;
                if nombre == SO_RCVTIMEO {
                    self.zm(z).rcvtimeo_us = us;
                } else {
                    self.zm(z).sndtimeo_us = us;
                }
            }
            (SOL_SOCKET, SO_EVENTMASK) => {
                let zz = self.zm(z);
                zz.eventmask = v;
                zz.antes = 0;
            }
            (SOL_SOCKET, SO_ERROR | SO_TYPE | SO_ACCEPTCONN) => return errno(ENOPROTOOPT),
            (
                SOL_SOCKET,
                SO_DEBUG | SO_REUSEADDR | SO_KEEPALIVE | SO_DONTROUTE | SO_BROADCAST | SO_USELOOPBACK | SO_OOBINLINE
                | SO_REUSEPORT | SO_SNDBUF | SO_RCVBUF | SO_SNDLOWAT | SO_RCVLOWAT,
            )
            | (IPPROTO_IP, IP_TOS | IP_TTL) => {
                self.zm(z).opciones.insert((nivel, nombre), v);
            }
            (IPPROTO_TCP, TCP_NODELAY | TCP_MAXSEG) if tipo == Tipo::Flujo => {
                self.zm(z).opciones.insert((nivel, nombre), v);
            }
            _ => return errno(ENOPROTOOPT),
        }
        Ok(0)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn getsockopt<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        base: u32,
        fd: u32,
        nivel: u32,
        nombre: u32,
        valor: u32,
        lenp: u32,
    ) -> Res<u32> {
        use sock::*;
        let z = self.zocalo_de(base, fd)?;
        if valor == 0 || lenp == 0 {
            return Err(Mal::Fallo(Fallo::PunteroNulo));
        }
        let largo = l32(ex, lenp)? as i32;
        let dos = |ex: &mut Exec<M>, a: u32, b: u32| -> Res<u32> {
            if largo < 8 {
                return errno(EINVAL);
            }
            tramo(ex, valor, 8, true)?;
            e32(ex, valor, a)?;
            e32(ex, valor + 4, b)?;
            e32(ex, lenp, 8)?;
            Ok(0)
        };
        let tipo = self.z(z).tipo;
        let v = match (nivel, nombre) {
            (SOL_SOCKET, SO_LINGER) => {
                let (a, b) = self.z(z).linger;
                return dos(ex, a, b);
            }
            (SOL_SOCKET, SO_RCVTIMEO) | (SOL_SOCKET, SO_SNDTIMEO) => {
                let us = if nombre == SO_RCVTIMEO { self.z(z).rcvtimeo_us } else { self.z(z).sndtimeo_us };
                return dos(ex, (us / 1_000_000) as u32, (us % 1_000_000) as u32);
            }
            (SOL_SOCKET, SO_ERROR) => {
                self.mirar(red, z);
                core::mem::take(&mut self.zm(z).error) as u32
            }
            (SOL_SOCKET, SO_TYPE) => match tipo {
                Tipo::Flujo => SOCK_STREAM,
                Tipo::Datagrama => SOCK_DGRAM,
            },
            (SOL_SOCKET, SO_ACCEPTCONN) => matches!(self.z(z).estado, Estado::Escuchando { .. }) as u32,
            (SOL_SOCKET, SO_EVENTMASK) => self.z(z).eventmask,
            (SOL_SOCKET, SO_SNDBUF | SO_RCVBUF) => *self.z(z).opciones.get(&(nivel, nombre)).unwrap_or(&16384),
            (
                SOL_SOCKET,
                SO_DEBUG | SO_REUSEADDR | SO_KEEPALIVE | SO_DONTROUTE | SO_BROADCAST | SO_USELOOPBACK | SO_OOBINLINE
                | SO_REUSEPORT | SO_SNDLOWAT | SO_RCVLOWAT,
            )
            | (IPPROTO_IP, IP_TOS | IP_TTL) => *self.z(z).opciones.get(&(nivel, nombre)).unwrap_or(&0),
            (IPPROTO_TCP, TCP_NODELAY | TCP_MAXSEG) if tipo == Tipo::Flujo => {
                let def = if nombre == TCP_MAXSEG { 1460 } else { 0 };
                *self.z(z).opciones.get(&(nivel, nombre)).unwrap_or(&def)
            }
            _ => return errno(ENOPROTOOPT),
        };
        if largo < 4 {
            return errno(EINVAL);
        }
        e32(ex, valor, v)?;
        e32(ex, lenp, 4)?;
        Ok(0)
    }

    /// `getsockname` (`par` falso) y `getpeername` (`par` cierto).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn nombre_de<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        base: u32,
        fd: u32,
        addr: u32,
        lenp: u32,
        par: bool,
    ) -> Res<u32> {
        let z = self.zocalo_de(base, fd)?;
        if addr == 0 || lenp == 0 {
            return Err(Mal::Fallo(Fallo::PunteroNulo));
        }
        self.mirar(red, z);
        let zz = self.z(z);
        let (ip, puerto) = if par {
            match zz.estado {
                Estado::Conectado(c) => match red.extremos(c) {
                    Some(e) if e.remoto_puerto != 0 => (e.remoto, e.remoto_puerto),
                    _ => zz.remoto.unwrap_or(([0; 4], 0)),
                },
                Estado::Datagramas(_) if zz.remoto.is_some() => zz.remoto.unwrap(),
                _ => return errno(ENOTCONN),
            }
        } else {
            let ip = if zz.local.0 == [0; 4] && matches!(zz.estado, Estado::Conectado(_)) {
                red.direccion_local()
            } else {
                zz.local.0
            };
            match zz.estado {
                Estado::Conectado(c) | Estado::Datagramas(c) => {
                    (ip, red.extremos(c).map_or(zz.local.1, |e| e.local_puerto))
                }
                Estado::Escuchando { puerto, .. } => (ip, puerto),
                _ => zz.local,
            }
        };
        poner_sockaddr(ex, addr, lenp, ip, puerto)?;
        Ok(0)
    }

    pub(crate) fn ioctl<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        base: u32,
        fd: u32,
        peticion: u32,
        arg: u32,
    ) -> Res<u32> {
        let z = self.zocalo_de(base, fd)?;
        let leer = |ex: &Exec<M>| -> Res<u32> {
            if arg == 0 {
                return Err(Mal::Fallo(Fallo::PunteroNulo));
            }
            Ok(l32(ex, arg)?)
        };
        match peticion {
            fio::FIONBIO => {
                let v = leer(ex)?;
                self.zm(z).no_bloqueante = v != 0;
            }
            fio::FIOASYNC => {
                let v = leer(ex)?;
                let zz = self.zm(z);
                zz.asincrono = v != 0;
                zz.dueno_sigio = base;
            }
            fio::FIONREAD => {
                if arg == 0 {
                    return Err(Mal::Fallo(Fallo::PunteroNulo));
                }
                self.mirar(red, z);
                let zz = self.z(z);
                let n = match zz.estado {
                    Estado::Conectado(c) => zz.guardado.len() + red.pendiente(c).0,
                    Estado::Datagramas(u) => match &zz.datagrama {
                        Some(d) => d.0.len(),
                        None => red.pendiente(u).0.saturating_sub(24),
                    },
                    _ => 0,
                };
                e32(ex, arg, n as u32)?;
            }
            fio::SIOCATMARK | fio::FIOGETOWN => {
                if arg == 0 {
                    return Err(Mal::Fallo(Fallo::PunteroNulo));
                }
                let v =
                    if peticion == fio::FIOGETOWN { *self.z(z).opciones.get(&(0, peticion)).unwrap_or(&0) } else { 0 };
                e32(ex, arg, v)?;
            }
            fio::FIOSETOWN => {
                let v = leer(ex)?;
                self.zm(z).opciones.insert((0, fio::FIOGETOWN), v);
            }
            fio::FIOCLEX | fio::FIONCLEX => {}
            _ => {
                self.avisar(ex, "IoctlSocket", Fallo::NoImplementada { n: peticion as u16 });
                return errno(EINVAL);
            }
        }
        Ok(0)
    }

    pub(crate) fn close_socket<R: Red>(&mut self, red: &mut R, base: u32, fd: u32) -> Res<u32> {
        let z = self.zocalo_de(base, fd)?;
        self.base_mut(base).fds[fd as usize] = None;
        self.soltar_zocalo(red, z);
        Ok(0)
    }

    // --- WaitSelect ---

    /// `WaitSelect(nfds, read, write, except, timeout, signals)(d0/a0/a1/a2/a3/d1)`.
    pub(crate) fn wait_select<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        ll: &Llamada,
    ) -> Res<Hecho> {
        let previo = self.reanudar(ex, red, ll);
        let (nfds, conjuntos, tv, sigp) = (ll.d[0] as i32, [ll.a[0], ll.a[1], ll.a[2]], ll.a[3], ll.d[1]);
        if nfds < 0 {
            return errno(EINVAL);
        }
        // Lo que pasa del tamaño de la tabla se recorta, como dice el autodoc.
        let nfds = (nfds as usize).min(self.base_ref(ll.base).fds.len()) as u32;
        let palabras = nfds.div_ceil(32);
        let mut pedidos = [Vec::new(), Vec::new(), Vec::new()];
        for (k, &c) in conjuntos.iter().enumerate() {
            if c == 0 {
                continue;
            }
            tramo(ex, c, palabras * 4, true)?;
            for w in 0..palabras {
                pedidos[k].push(l32(ex, c + 4 * w)?);
            }
        }
        let vence = match &previo {
            Some(b) => b.vence,
            None if tv != 0 => {
                let s = l32(ex, tv + timeval::TV_SECS)?;
                let us = l32(ex, tv + timeval::TV_MICRO)?;
                if us >= 1_000_000 || s > 100_000_000 {
                    return errno(EINVAL);
                }
                Some(ll.ahora.saturating_add(s as u64 * 1_000_000 + us as u64))
            }
            None => None,
        };
        let usuario = if sigp != 0 { l32(ex, sigp)? } else { 0 };
        // Qué zócalo y qué se pide de cada descriptor.
        let mut lista: Vec<(u32, u32, u8)> = Vec::new();
        for fd in 0..nfds {
            let (w, bit) = ((fd / 32) as usize, 1u32 << (fd % 32));
            let mut q = 0;
            for (k, p) in pedidos.iter().enumerate() {
                if !p.is_empty() && p[w] & bit != 0 {
                    q |= 1 << k;
                }
            }
            if q != 0 {
                lista.push((fd, self.zocalo_de(ll.base, fd)?, q));
            }
        }
        let mut listos = [
            alloc::vec![0u32; palabras as usize],
            alloc::vec![0; palabras as usize],
            alloc::vec![0; palabras as usize],
        ];
        let mut cuantos = 0;
        for &(fd, z, q) in &lista {
            let l = self.mirar(red, z) & q;
            for (k, s) in listos.iter_mut().enumerate() {
                if l & (1 << k) != 0 {
                    s[(fd / 32) as usize] |= 1 << (fd % 32);
                    cuantos += 1;
                }
            }
        }
        let recibidas = self.senales(ex)?;
        let breakmask = self.base_ref(ll.base).breakmask;
        if recibidas & breakmask & !usuario != 0 {
            // Ctrl-C: -1 y EINTR, los conjuntos sin tocar y la señal sigue puesta.
            return errno(EINTR);
        }
        let del_usuario = recibidas & usuario;
        let vencido = vence.is_some_and(|v| v <= ll.ahora);
        if cuantos > 0 || del_usuario != 0 || vencido {
            for (k, &c) in conjuntos.iter().enumerate() {
                if c != 0 {
                    for (w, v) in listos[k].iter().enumerate() {
                        e32(ex, c + 4 * w as u32, *v)?;
                    }
                }
            }
            if sigp != 0 {
                e32(ex, sigp, del_usuario)?;
                ex.set_signal(0, del_usuario)?;
            }
            return Ok(Hecho::Valor(cuantos));
        }
        let cond = Condicion::Seleccion(lista.iter().map(|&(_, z, q)| (z, q)).collect());
        self.dormir(ex, ll, Bloqueo::nuevo(ll, cond, vence), usuario)
    }

    // --- Pasar zócalos ---

    pub(crate) fn dup2<R: Red>(&mut self, red: &mut R, base: u32, viejo: u32, nuevo: u32) -> Res<u32> {
        let z = self.zocalo_de(base, viejo)?;
        if nuevo == u32::MAX {
            return self.ocupar(base, z, true);
        }
        if nuevo as usize >= self.base_ref(base).fds.len() {
            return errno(EBADF);
        }
        if nuevo == viejo {
            return Ok(nuevo);
        }
        if let Some(x) = self.base_mut(base).fds[nuevo as usize].take() {
            self.soltar_zocalo(red, x);
        }
        self.base_mut(base).fds[nuevo as usize] = Some(z);
        self.zm(z).refs += 1;
        Ok(nuevo)
    }

    pub(crate) fn release_socket(&mut self, base: u32, fd: u32, id: u32, copia: bool) -> Res<u32> {
        let z = self.zocalo_de(base, fd)?;
        if self.publicos.len() >= 64 {
            return errno(ENOMEM);
        }
        let id = if id == sbt::UNIQUE_ID {
            loop {
                let u = self.unico;
                self.unico = self.unico.wrapping_add(1).max(0x10000);
                if !self.publicos.iter().any(|p| p.id == u) {
                    break u;
                }
            }
        } else {
            if id > 0xFFFF && self.publicos.iter().any(|p| p.id == id) {
                return errno(EINVAL);
            }
            id
        };
        if copia {
            self.zm(z).refs += 1;
        } else {
            self.base_mut(base).fds[fd as usize] = None;
        }
        self.publicos.push(Publico { id, zocalo: z });
        Ok(id)
    }

    pub(crate) fn obtain_socket(&mut self, base: u32, id: u32, dominio: u32, tipo: u32, protocolo: u32) -> Res<u32> {
        if dominio != sock::AF_INET {
            return errno(EPROTONOSUPPORT);
        }
        let quiere = match tipo {
            sock::SOCK_STREAM => Tipo::Flujo,
            sock::SOCK_DGRAM => Tipo::Datagrama,
            _ => return errno(EPROTONOSUPPORT),
        };
        let i = self.publicos.iter().position(|p| {
            p.id == id
                && (id > 0xFFFF
                    || self
                        .zocalos
                        .get(&p.zocalo)
                        .is_some_and(|z| z.tipo == quiere && (protocolo == 0 || protocolo == z.protocolo)))
        });
        let Some(i) = i else { return errno(EBADF) };
        let z = self.publicos[i].zocalo;
        let fd = self.ocupar(base, z, false)?;
        self.publicos.remove(i);
        Ok(fd)
    }

    // --- Avisos asíncronos ---

    /// Mira un zócalo y apunta en `eventos` lo que acaba de pasar (`FD_READ`, `FD_ACCEPT`...). Devuelve lo nuevo.
    fn actualizar_eventos<R: Red>(&mut self, red: &mut R, z: u32) -> u32 {
        let Some(antes) = self.zocalos.get(&z).map(|x| x.eventos) else { return 0 };
        let l = self.mirar(red, z);
        let zz = self.zocalos.get_mut(&z).unwrap();
        let mut ahora = 0;
        match zz.estado {
            Estado::Conectado(c) => {
                let f = red.fase(c);
                if !zz.guardado.is_empty() || red.pendiente(c).0 > 0 {
                    ahora |= sbt::FD_READ;
                }
                match f {
                    Fase::Fin => ahora |= sbt::FD_CLOSE,
                    Fase::Error(_) => ahora |= sbt::FD_CLOSE | sbt::FD_ERROR,
                    _ => {
                        if l & ESCRIBIR != 0 {
                            ahora |= sbt::FD_WRITE;
                        }
                    }
                }
            }
            Estado::Escuchando { ref entradas, .. } => {
                if !entradas.is_empty() {
                    ahora |= sbt::FD_ACCEPT;
                }
            }
            Estado::Datagramas(_) => {
                if l & LEER != 0 {
                    ahora |= sbt::FD_READ;
                }
                if l & ESCRIBIR != 0 {
                    ahora |= sbt::FD_WRITE;
                }
            }
            _ => {}
        }
        let flancos = ahora & !zz.antes;
        zz.antes = ahora;
        zz.eventos |= flancos;
        zz.eventos & !antes
    }

    /// Las señales de `FIOASYNC` (`SBTC_SIGIOMASK`) y `SO_EVENTMASK` (`SBTC_SIGEVENTMASK`) a la tarea de cada base que
    /// tiene el zócalo.
    pub(crate) fn avisos_asincronos<M: Memoria, R: Red>(&mut self, ex: &mut Exec<M>, red: &mut R) {
        let cuales: Vec<u32> =
            self.zocalos.iter().filter(|(_, z)| z.eventmask != 0 || z.asincrono).map(|(k, _)| *k).collect();
        for z in cuales {
            let nuevos = self.actualizar_eventos(red, z);
            if nuevos == 0 {
                continue;
            }
            let (mascara, asincrono) = {
                let zz = self.z(z);
                (zz.eventmask, zz.asincrono)
            };
            let a_quien: Vec<(u32, u32, u32)> = self
                .bases
                .values()
                .filter(|b| b.fds.contains(&Some(z)))
                .map(|b| (b.tarea, b.sigevent, b.sigio))
                .collect();
            for (t, sigevent, sigio) in a_quien {
                if nuevos & mascara != 0 && sigevent != 0 {
                    let _ = ex.signal(t, sigevent);
                }
                if asincrono && sigio != 0 {
                    let _ = ex.signal(t, sigio);
                }
            }
        }
    }

    pub(crate) fn get_socket_events<M: Memoria, R: Red>(
        &mut self,
        ex: &mut Exec<M>,
        red: &mut R,
        base: u32,
        ptr: u32,
    ) -> Res<u32> {
        if ptr == 0 {
            return Err(Mal::Fallo(Fallo::PunteroNulo));
        }
        tramo(ex, ptr, 4, true)?;
        let fds = self.base_ref(base).fds.clone();
        for (fd, z) in fds.iter().enumerate() {
            let Some(z) = *z else { continue };
            self.actualizar_eventos(red, z);
            let zz = self.zm(z);
            let p = zz.eventos & zz.eventmask;
            if p != 0 {
                zz.eventos &= !p;
                e32(ex, ptr, p)?;
                return Ok(fd as u32);
            }
        }
        Ok(u32::MAX)
    }
}
