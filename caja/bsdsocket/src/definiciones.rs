//! Lo que fija la interfaz: los números y las estructuras de los includes del NDK 3.2 (`libraries/bsdsocket.h`,
//! `sys/socket.h`, `netinet/in.h`, `netdb.h`, `sys/filio.h`) y los valores de `errno` de BSD 4.4 que usan AmiTCP y
//! Roadshow. `tests/estructuras.rs` los coteja con los includes.

/// Qué devuelve una función cuando falla o no está hecha: -1, 0 (las que dan un puntero o un BOOL) o nada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallida {
    MenosUno,
    Cero,
    Nada,
}

/// Las funciones de bsdsocket.library, en el orden de la tabla de saltos desde -30 (`pragmas/bsdsocket_pragmas.h`):
/// las de AmiTCP 4 hasta `GetSocketEvents` (-300), diez reservadas y las de Roadshow hasta `getnameinfo` (-822).
pub const FUNCIONES: [(&str, Fallida); 133] = [
    ("socket", Fallida::MenosUno),
    ("bind", Fallida::MenosUno),
    ("listen", Fallida::MenosUno),
    ("accept", Fallida::MenosUno),
    ("connect", Fallida::MenosUno),
    ("sendto", Fallida::MenosUno),
    ("send", Fallida::MenosUno),
    ("recvfrom", Fallida::MenosUno),
    ("recv", Fallida::MenosUno),
    ("shutdown", Fallida::MenosUno),
    ("setsockopt", Fallida::MenosUno),
    ("getsockopt", Fallida::MenosUno),
    ("getsockname", Fallida::MenosUno),
    ("getpeername", Fallida::MenosUno),
    ("IoctlSocket", Fallida::MenosUno),
    ("CloseSocket", Fallida::MenosUno),
    ("WaitSelect", Fallida::MenosUno),
    ("SetSocketSignals", Fallida::Nada),
    ("getdtablesize", Fallida::MenosUno),
    ("ObtainSocket", Fallida::MenosUno),
    ("ReleaseSocket", Fallida::MenosUno),
    ("ReleaseCopyOfSocket", Fallida::MenosUno),
    ("Errno", Fallida::MenosUno),
    ("SetErrnoPtr", Fallida::Nada),
    ("Inet_NtoA", Fallida::Cero),
    ("inet_addr", Fallida::MenosUno),
    ("Inet_LnaOf", Fallida::MenosUno),
    ("Inet_NetOf", Fallida::MenosUno),
    ("Inet_MakeAddr", Fallida::MenosUno),
    ("inet_network", Fallida::MenosUno),
    ("gethostbyname", Fallida::Cero),
    ("gethostbyaddr", Fallida::Cero),
    ("getnetbyname", Fallida::Cero),
    ("getnetbyaddr", Fallida::Cero),
    ("getservbyname", Fallida::Cero),
    ("getservbyport", Fallida::Cero),
    ("getprotobyname", Fallida::Cero),
    ("getprotobynumber", Fallida::Cero),
    ("vsyslog", Fallida::Nada),
    ("Dup2Socket", Fallida::MenosUno),
    ("sendmsg", Fallida::MenosUno),
    ("recvmsg", Fallida::MenosUno),
    ("gethostname", Fallida::MenosUno),
    ("gethostid", Fallida::MenosUno),
    ("SocketBaseTagList", Fallida::MenosUno),
    ("GetSocketEvents", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("bpf_open", Fallida::MenosUno),
    ("bpf_close", Fallida::MenosUno),
    ("bpf_read", Fallida::MenosUno),
    ("bpf_write", Fallida::MenosUno),
    ("bpf_set_notify_mask", Fallida::MenosUno),
    ("bpf_set_interrupt_mask", Fallida::MenosUno),
    ("bpf_ioctl", Fallida::MenosUno),
    ("bpf_data_waiting", Fallida::MenosUno),
    ("AddRouteTagList", Fallida::MenosUno),
    ("DeleteRouteTagList", Fallida::MenosUno),
    ("ChangeRouteTagList", Fallida::MenosUno),
    ("FreeRouteInfo", Fallida::Nada),
    ("GetRouteInfo", Fallida::Cero),
    ("AddInterfaceTagList", Fallida::MenosUno),
    ("ConfigureInterfaceTagList", Fallida::MenosUno),
    ("ReleaseInterfaceList", Fallida::Nada),
    ("ObtainInterfaceList", Fallida::Cero),
    ("QueryInterfaceTagList", Fallida::MenosUno),
    ("CreateAddrAllocMessageA", Fallida::MenosUno),
    ("DeleteAddrAllocMessage", Fallida::Nada),
    ("BeginInterfaceConfig", Fallida::Nada),
    ("AbortInterfaceConfig", Fallida::Nada),
    ("AddNetMonitorHookTagList", Fallida::MenosUno),
    ("RemoveNetMonitorHook", Fallida::Nada),
    ("GetNetworkStatistics", Fallida::MenosUno),
    ("AddDomainNameServer", Fallida::MenosUno),
    ("RemoveDomainNameServer", Fallida::MenosUno),
    ("ReleaseDomainNameServerList", Fallida::Nada),
    ("ObtainDomainNameServerList", Fallida::Cero),
    ("setnetent", Fallida::Nada),
    ("endnetent", Fallida::Nada),
    ("getnetent", Fallida::Cero),
    ("setprotoent", Fallida::Nada),
    ("endprotoent", Fallida::Nada),
    ("getprotoent", Fallida::Cero),
    ("setservent", Fallida::Nada),
    ("endservent", Fallida::Nada),
    ("getservent", Fallida::Cero),
    ("inet_aton", Fallida::MenosUno),
    ("inet_ntop", Fallida::Cero),
    ("inet_pton", Fallida::MenosUno),
    ("In_LocalAddr", Fallida::Cero),
    ("In_CanForward", Fallida::Cero),
    ("mbuf_copym", Fallida::Cero),
    ("mbuf_copyback", Fallida::MenosUno),
    ("mbuf_copydata", Fallida::MenosUno),
    ("mbuf_free", Fallida::Cero),
    ("mbuf_freem", Fallida::Nada),
    ("mbuf_get", Fallida::Cero),
    ("mbuf_gethdr", Fallida::Cero),
    ("mbuf_prepend", Fallida::Cero),
    ("mbuf_cat", Fallida::MenosUno),
    ("mbuf_adj", Fallida::MenosUno),
    ("mbuf_pullup", Fallida::Cero),
    ("ProcessIsServer", Fallida::Cero),
    ("ObtainServerSocket", Fallida::MenosUno),
    ("GetDefaultDomainName", Fallida::Cero),
    ("SetDefaultDomainName", Fallida::Nada),
    ("ObtainRoadshowData", Fallida::Cero),
    ("ReleaseRoadshowData", Fallida::Nada),
    ("ChangeRoadshowData", Fallida::Cero),
    ("RemoveInterface", Fallida::MenosUno),
    ("gethostbyname_r", Fallida::Cero),
    ("gethostbyaddr_r", Fallida::Cero),
    ("(reservada)", Fallida::MenosUno),
    ("(reservada)", Fallida::MenosUno),
    ("ipf_open", Fallida::MenosUno),
    ("ipf_close", Fallida::MenosUno),
    ("ipf_ioctl", Fallida::MenosUno),
    ("ipf_log_read", Fallida::MenosUno),
    ("ipf_log_data_waiting", Fallida::MenosUno),
    ("ipf_set_notify_mask", Fallida::MenosUno),
    ("ipf_set_interrupt_mask", Fallida::MenosUno),
    ("freeaddrinfo", Fallida::Nada),
    ("getaddrinfo", Fallida::MenosUno),
    ("gai_strerror", Fallida::Cero),
    ("getnameinfo", Fallida::MenosUno),
];

/// El LVO (positivo) de una función por su nombre.
pub fn lvo(nombre: &str) -> Option<u32> {
    FUNCIONES.iter().position(|(n, _)| *n == nombre).map(|i| 30 + 6 * i as u32)
}

/// `errno` (BSD 4.4, como `sys/errno.h` de AmiTCP y Roadshow).
pub mod errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const EINTR: i32 = 4;
    pub const EIO: i32 = 5;
    pub const EBADF: i32 = 9;
    pub const ENOMEM: i32 = 12;
    pub const EACCES: i32 = 13;
    pub const EFAULT: i32 = 14;
    pub const EINVAL: i32 = 22;
    pub const ENFILE: i32 = 23;
    pub const EMFILE: i32 = 24;
    pub const ENOTTY: i32 = 25;
    pub const ENOSPC: i32 = 28;
    pub const EPIPE: i32 = 32;
    pub const ERANGE: i32 = 34;
    pub const EAGAIN: i32 = 35;
    pub const EWOULDBLOCK: i32 = EAGAIN;
    pub const EINPROGRESS: i32 = 36;
    pub const EALREADY: i32 = 37;
    pub const ENOTSOCK: i32 = 38;
    pub const EDESTADDRREQ: i32 = 39;
    pub const EMSGSIZE: i32 = 40;
    pub const EPROTOTYPE: i32 = 41;
    pub const ENOPROTOOPT: i32 = 42;
    pub const EPROTONOSUPPORT: i32 = 43;
    pub const ESOCKTNOSUPPORT: i32 = 44;
    pub const EOPNOTSUPP: i32 = 45;
    pub const EPFNOSUPPORT: i32 = 46;
    pub const EAFNOSUPPORT: i32 = 47;
    pub const EADDRINUSE: i32 = 48;
    pub const EADDRNOTAVAIL: i32 = 49;
    pub const ENETDOWN: i32 = 50;
    pub const ENETUNREACH: i32 = 51;
    pub const ENETRESET: i32 = 52;
    pub const ECONNABORTED: i32 = 53;
    pub const ECONNRESET: i32 = 54;
    pub const ENOBUFS: i32 = 55;
    pub const EISCONN: i32 = 56;
    pub const ENOTCONN: i32 = 57;
    pub const ESHUTDOWN: i32 = 58;
    pub const ETIMEDOUT: i32 = 60;
    pub const ECONNREFUSED: i32 = 61;
    pub const EHOSTDOWN: i32 = 64;
    pub const EHOSTUNREACH: i32 = 65;
    pub const ENOSYS: i32 = 78;
}

/// `sys/socket.h`, `netinet/in.h`, `netinet/tcp.h`.
pub mod sock {
    pub const AF_UNSPEC: u32 = 0;
    pub const AF_INET: u32 = 2;
    pub const AF_INET6: u32 = 23;
    pub const SOCK_STREAM: u32 = 1;
    pub const SOCK_DGRAM: u32 = 2;
    pub const SOCK_RAW: u32 = 3;
    pub const IPPROTO_IP: u32 = 0;
    pub const IPPROTO_ICMP: u32 = 1;
    pub const IPPROTO_TCP: u32 = 6;
    pub const IPPROTO_UDP: u32 = 17;
    pub const SOL_SOCKET: u32 = 0xFFFF;
    pub const SO_DEBUG: u32 = 0x0001;
    pub const SO_ACCEPTCONN: u32 = 0x0002;
    pub const SO_REUSEADDR: u32 = 0x0004;
    pub const SO_KEEPALIVE: u32 = 0x0008;
    pub const SO_DONTROUTE: u32 = 0x0010;
    pub const SO_BROADCAST: u32 = 0x0020;
    pub const SO_USELOOPBACK: u32 = 0x0040;
    pub const SO_LINGER: u32 = 0x0080;
    pub const SO_OOBINLINE: u32 = 0x0100;
    pub const SO_REUSEPORT: u32 = 0x0200;
    pub const SO_SNDBUF: u32 = 0x1001;
    pub const SO_RCVBUF: u32 = 0x1002;
    pub const SO_SNDLOWAT: u32 = 0x1003;
    pub const SO_RCVLOWAT: u32 = 0x1004;
    pub const SO_SNDTIMEO: u32 = 0x1005;
    pub const SO_RCVTIMEO: u32 = 0x1006;
    pub const SO_ERROR: u32 = 0x1007;
    pub const SO_TYPE: u32 = 0x1008;
    pub const SO_EVENTMASK: u32 = 0x2001;
    pub const TCP_NODELAY: u32 = 1;
    pub const TCP_MAXSEG: u32 = 2;
    pub const IP_TOS: u32 = 3;
    pub const IP_TTL: u32 = 4;
    pub const MSG_OOB: u32 = 0x1;
    pub const MSG_PEEK: u32 = 0x2;
    pub const MSG_DONTROUTE: u32 = 0x4;
    pub const MSG_EOR: u32 = 0x8;
    pub const MSG_TRUNC: u32 = 0x10;
    pub const MSG_CTRUNC: u32 = 0x20;
    pub const MSG_WAITALL: u32 = 0x40;
    pub const MSG_DONTWAIT: u32 = 0x80;
    pub const SHUT_RD: u32 = 0;
    pub const SHUT_WR: u32 = 1;
    pub const SHUT_RDWR: u32 = 2;
    pub const SOMAXCONN: u32 = 5;
    pub const INADDR_ANY: u32 = 0;
    pub const INADDR_NONE: u32 = 0xFFFF_FFFF;
    /// `FD_SETSIZE` de `sys/socket.h` del NDK; el `sys/types.h` del compilador o el programa pueden poner otro. La
    /// biblioteca no depende de él: lee lo que dice `nfds`.
    pub const FD_SETSIZE: u32 = 256;
}

/// Las peticiones de `IoctlSocket` (`sys/filio.h`, `sys/sockio.h`): `_IOC(sentido, grupo, número, largo)` de
/// `sys/ioccom.h`.
pub mod fio {
    const fn ioc(sentido: u32, grupo: u8, n: u8, largo: u32) -> u32 {
        sentido | ((largo & 0x1FFF) << 16) | ((grupo as u32) << 8) | n as u32
    }
    const IOC_VOID: u32 = 0x2000_0000;
    const IOC_OUT: u32 = 0x4000_0000;
    const IOC_IN: u32 = 0x8000_0000;
    pub const FIOCLEX: u32 = ioc(IOC_VOID, b'f', 1, 0);
    pub const FIONCLEX: u32 = ioc(IOC_VOID, b'f', 2, 0);
    pub const FIONREAD: u32 = ioc(IOC_OUT, b'f', 127, 4);
    pub const FIONBIO: u32 = ioc(IOC_IN, b'f', 126, 4);
    pub const FIOASYNC: u32 = ioc(IOC_IN, b'f', 125, 4);
    pub const FIOSETOWN: u32 = ioc(IOC_IN, b'f', 124, 4);
    pub const FIOGETOWN: u32 = ioc(IOC_OUT, b'f', 123, 4);
    pub const SIOCATMARK: u32 = ioc(IOC_OUT, b's', 7, 4);
}

/// `libraries/bsdsocket.h`: `SocketBaseTagList` y `GetSocketEvents`.
pub mod sbt {
    pub const TAG_USER: u32 = 0x8000_0000;
    pub const SBTF_REF: u32 = 0x8000;
    pub const SBTF_SET: u32 = 1;
    pub const SBTB_CODE: u32 = 1;
    pub const SBTS_CODE: u32 = 0x3FFF;
    pub const fn getval(c: u32) -> u32 {
        TAG_USER | ((c & SBTS_CODE) << SBTB_CODE)
    }
    pub const fn getref(c: u32) -> u32 {
        TAG_USER | SBTF_REF | ((c & SBTS_CODE) << SBTB_CODE)
    }
    pub const fn setval(c: u32) -> u32 {
        TAG_USER | ((c & SBTS_CODE) << SBTB_CODE) | SBTF_SET
    }
    pub const fn setref(c: u32) -> u32 {
        TAG_USER | SBTF_REF | ((c & SBTS_CODE) << SBTB_CODE) | SBTF_SET
    }
    pub const SBTC_BREAKMASK: u32 = 1;
    pub const SBTC_SIGIOMASK: u32 = 2;
    pub const SBTC_SIGURGMASK: u32 = 3;
    pub const SBTC_SIGEVENTMASK: u32 = 4;
    pub const SBTC_ERRNO: u32 = 6;
    pub const SBTC_HERRNO: u32 = 7;
    pub const SBTC_DTABLESIZE: u32 = 8;
    pub const SBTC_FDCALLBACK: u32 = 9;
    pub const SBTC_LOGSTAT: u32 = 10;
    pub const SBTC_LOGTAGPTR: u32 = 11;
    pub const SBTC_LOGFACILITY: u32 = 12;
    pub const SBTC_LOGMASK: u32 = 13;
    pub const SBTC_ERRNOSTRPTR: u32 = 14;
    pub const SBTC_HERRNOSTRPTR: u32 = 15;
    pub const SBTC_IOERRNOSTRPTR: u32 = 16;
    pub const SBTC_S2ERRNOSTRPTR: u32 = 17;
    pub const SBTC_S2WERRNOSTRPTR: u32 = 18;
    pub const SBTC_ERRNOBYTEPTR: u32 = 21;
    pub const SBTC_ERRNOWORDPTR: u32 = 22;
    pub const SBTC_ERRNOLONGPTR: u32 = 24;
    pub const SBTC_HERRNOLONGPTR: u32 = 25;
    pub const SBTC_RELEASESTRPTR: u32 = 29;
    pub const SBTC_NUM_PACKET_FILTER_CHANNELS: u32 = 40;
    pub const SBTC_HAVE_ROUTING_API: u32 = 41;
    pub const SBTC_HAVE_INTERFACE_API: u32 = 47;
    pub const SBTC_HAVE_MONITORING_API: u32 = 50;
    pub const SBTC_CAN_SHARE_LIBRARY_BASES: u32 = 51;
    pub const SBTC_HAVE_STATUS_API: u32 = 53;
    pub const SBTC_HAVE_DNS_API: u32 = 54;
    pub const SBTC_SYSTEM_STATUS: u32 = 56;
    pub const SBTC_SIG_ADDRESS_CHANGE_MASK: u32 = 57;
    pub const SBTC_IPF_API_VERSION: u32 = 58;
    pub const SBTC_HAVE_LOCAL_DATABASE_API: u32 = 59;
    pub const SBTC_HAVE_ADDRESS_CONVERSION_API: u32 = 60;
    pub const SBTC_HAVE_KERNEL_MEMORY_API: u32 = 61;
    pub const SBTC_HAVE_SERVER_API: u32 = 63;
    pub const SBTC_GET_BYTES_RECEIVED: u32 = 64;
    pub const SBTC_GET_BYTES_SENT: u32 = 65;
    pub const SBTC_HAVE_ROADSHOWDATA_API: u32 = 67;
    pub const SBTC_HAVE_GETHOSTADDR_R_API: u32 = 69;
    pub const SBSYSSTAT_INTERFACES: u32 = 1 << 0;
    pub const SBSYSSTAT_BCAST_INTERFACES: u32 = 1 << 2;
    pub const SBSYSSTAT_RESOLVER: u32 = 1 << 3;
    pub const SBSYSSTAT_ROUTES: u32 = 1 << 4;
    pub const SBSYSSTAT_DEFAULTROUTE: u32 = 1 << 5;
    /// `ReleaseSocket(fd, UNIQUE_ID)`.
    pub const UNIQUE_ID: u32 = 0xFFFF_FFFF;
    pub const FD_ACCEPT: u32 = 0x01;
    pub const FD_CONNECT: u32 = 0x02;
    pub const FD_OOB: u32 = 0x04;
    pub const FD_READ: u32 = 0x08;
    pub const FD_WRITE: u32 = 0x10;
    pub const FD_ERROR: u32 = 0x20;
    pub const FD_CLOSE: u32 = 0x40;
}

/// `netdb.h`.
pub mod netdb {
    pub const NETDB_INTERNAL: i32 = -1;
    pub const NETDB_SUCCESS: i32 = 0;
    pub const HOST_NOT_FOUND: i32 = 1;
    pub const TRY_AGAIN: i32 = 2;
    pub const NO_RECOVERY: i32 = 3;
    pub const NO_DATA: i32 = 4;
    pub const MAXHOSTNAMELEN: u32 = 256;
    pub const AI_PASSIVE: u32 = 1;
    pub const AI_CANONNAME: u32 = 2;
    pub const AI_NUMERICHOST: u32 = 4;
    pub const AI_EXT: u32 = 8;
    pub const AI_NUMERICSERV: u32 = 16;
    pub const NI_NUMERICHOST: u32 = 1;
    pub const NI_NUMERICSERV: u32 = 2;
    pub const NI_NOFQDN: u32 = 4;
    pub const NI_NAMEREQD: u32 = 8;
    pub const NI_DGRAM: u32 = 16;
    pub const EAI_BADFLAGS: i32 = -1;
    pub const EAI_NONAME: i32 = -2;
    pub const EAI_AGAIN: i32 = -3;
    pub const EAI_FAIL: i32 = -4;
    pub const EAI_NODATA: i32 = -5;
    pub const EAI_FAMILY: i32 = -6;
    pub const EAI_SOCKTYPE: i32 = -7;
    pub const EAI_SERVICE: i32 = -8;
    pub const EAI_ADDRFAMILY: i32 = -9;
    pub const EAI_MEMORY: i32 = -10;
    pub const EAI_SYSTEM: i32 = -11;
    pub const EAI_BADHINTS: i32 = -12;
    pub const EAI_PROTOCOL: i32 = -13;
}

/// `struct sockaddr_in` (BSD 4.4: `sin_len` y `sin_family` de un byte).
pub mod sockaddr_in {
    pub const SIN_LEN: u32 = 0;
    pub const SIN_FAMILY: u32 = 1;
    pub const SIN_PORT: u32 = 2;
    pub const SIN_ADDR: u32 = 4;
    pub const SIN_ZERO: u32 = 8;
    pub const TAMANO: u32 = 16;
}

pub mod hostent {
    pub const H_NAME: u32 = 0;
    pub const H_ALIASES: u32 = 4;
    pub const H_ADDRTYPE: u32 = 8;
    pub const H_LENGTH: u32 = 12;
    pub const H_ADDR_LIST: u32 = 16;
    pub const TAMANO: u32 = 20;
}

pub mod netent {
    pub const N_NAME: u32 = 0;
    pub const N_ALIASES: u32 = 4;
    pub const N_ADDRTYPE: u32 = 8;
    pub const N_NET: u32 = 12;
    pub const TAMANO: u32 = 16;
}

pub mod servent {
    pub const S_NAME: u32 = 0;
    pub const S_ALIASES: u32 = 4;
    pub const S_PORT: u32 = 8;
    pub const S_PROTO: u32 = 12;
    pub const TAMANO: u32 = 16;
}

pub mod protoent {
    pub const P_NAME: u32 = 0;
    pub const P_ALIASES: u32 = 4;
    pub const P_PROTO: u32 = 8;
    pub const TAMANO: u32 = 12;
}

/// `struct timeval` de `devices/timer.h` (`tv_secs`, `tv_micro`).
pub mod timeval {
    pub const TV_SECS: u32 = 0;
    pub const TV_MICRO: u32 = 4;
    pub const TAMANO: u32 = 8;
}

pub mod linger {
    pub const L_ONOFF: u32 = 0;
    pub const L_LINGER: u32 = 4;
    pub const TAMANO: u32 = 8;
}

pub mod msghdr {
    pub const MSG_NAME: u32 = 0;
    pub const MSG_NAMELEN: u32 = 4;
    pub const MSG_IOV: u32 = 8;
    pub const MSG_IOVLEN: u32 = 12;
    pub const MSG_CONTROL: u32 = 16;
    pub const MSG_CONTROLLEN: u32 = 20;
    pub const MSG_FLAGS: u32 = 24;
    pub const TAMANO: u32 = 28;
}

pub mod iovec {
    pub const IOV_BASE: u32 = 0;
    pub const IOV_LEN: u32 = 4;
    pub const TAMANO: u32 = 8;
}

pub mod addrinfo {
    pub const AI_FLAGS: u32 = 0;
    pub const AI_FAMILY: u32 = 4;
    pub const AI_SOCKTYPE: u32 = 8;
    pub const AI_PROTOCOL: u32 = 12;
    pub const AI_ADDRLEN: u32 = 16;
    pub const AI_ADDR: u32 = 20;
    pub const AI_CANONNAME: u32 = 24;
    pub const AI_NEXT: u32 = 28;
    pub const TAMANO: u32 = 32;
}

/// `struct TagItem` (`utility/tagitem.h`).
pub mod tagitem {
    pub const TI_TAG: u32 = 0;
    pub const TI_DATA: u32 = 4;
    pub const TAMANO: u32 = 8;
    pub const TAG_DONE: u32 = 0;
    pub const TAG_IGNORE: u32 = 1;
    pub const TAG_MORE: u32 = 2;
    pub const TAG_SKIP: u32 = 3;
}

/// Lo que va en cada base después de su `struct Library`: los datos que el autodoc dice que se reutilizan en cada
/// llamada (la `hostent` de `gethostbyname`, la cadena de `Inet_NtoA`...). Desplazamientos desde la base.
pub mod privado {
    pub const HOSTENT: u32 = 36;
    /// Una lista de alias vacía (un puntero nulo): la de todas las estructuras.
    pub const SIN_ALIAS: u32 = 56;
    /// `h_addr_list`: hasta 8 direcciones y el nulo.
    pub const ADDR_LIST: u32 = 60;
    pub const MAX_DIRECCIONES: u32 = 8;
    pub const DIRECCIONES: u32 = 96;
    pub const SERVENT: u32 = 128;
    pub const PROTOENT: u32 = 144;
    pub const NETENT: u32 = 156;
    pub const NTOA: u32 = 172;
    pub const S_NAME: u32 = 188;
    pub const S_PROTO: u32 = 220;
    pub const P_NAME: u32 = 228;
    pub const N_NAME: u32 = 244;
    pub const H_NAME: u32 = 260;
    pub const TAMANO: u32 = 516;
}
