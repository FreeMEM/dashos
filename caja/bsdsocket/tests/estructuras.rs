//! Los tamaños, desplazamientos y números de la biblioteca, cotejados con los includes del NDK 3.2: la tabla de saltos
//! con `pragmas/bsdsocket_pragmas.h`, y lo demás compilando aserciones con el compilador del 68k (si está).

use std::path::Path;
use std::process::Command;

use caja_bsdsocket::definiciones::*;

const NDK: &str = "/opt/amiga/m68k-amigaos/ndk-include";
const GCC: &str = "/opt/amiga/bin/m68k-amigaos-gcc";

#[test]
fn tabla_de_saltos_como_los_pragmas() {
    let p = Path::new(NDK).join("pragmas/bsdsocket_pragmas.h");
    let Ok(texto) = std::fs::read(&p) else {
        println!("sin {}: no se coteja", p.display());
        return;
    };
    let texto = String::from_utf8_lossy(&texto);
    let mut vistas = 0;
    for l in texto.lines().filter(|l| l.starts_with("#pragma amicall(SocketBase,0x")) {
        let resto = &l["#pragma amicall(SocketBase,0x".len()..];
        let (hex, resto) = resto.split_once(',').unwrap();
        let nombre = resto.split('(').next().unwrap();
        let lvo = u32::from_str_radix(hex, 16).unwrap();
        let i = ((lvo - 30) / 6) as usize;
        assert_eq!((lvo - 30) % 6, 0);
        assert_eq!(FUNCIONES[i].0, nombre, "LVO -{lvo}");
        assert_eq!(caja_bsdsocket::definiciones::lvo(nombre), Some(lvo));
        vistas += 1;
    }
    // Las demás entradas de la tabla son las reservadas.
    assert_eq!(vistas + FUNCIONES.iter().filter(|f| f.0 == "(reservada)").count(), FUNCIONES.len());
    assert_eq!(30 + 6 * (FUNCIONES.len() as u32 - 1), 0x336, "acaba en getnameinfo");
}

#[test]
fn zona_privada_de_la_base_sin_solapes() {
    use privado::*;
    let tramos = [
        (HOSTENT, hostent::TAMANO),
        (SIN_ALIAS, 4),
        (ADDR_LIST, 4 * (MAX_DIRECCIONES + 1)),
        (DIRECCIONES, 4 * MAX_DIRECCIONES),
        (SERVENT, servent::TAMANO),
        (PROTOENT, protoent::TAMANO),
        (NETENT, netent::TAMANO),
        (NTOA, 16),
        (S_NAME, 32),
        (S_PROTO, 8),
        (P_NAME, 16),
        (N_NAME, 16),
        (H_NAME, netdb::MAXHOSTNAMELEN),
    ];
    assert!(tramos[0].0 >= 34, "detrás de la struct Library");
    for w in tramos.windows(2) {
        assert!(w[0].0 + w[0].1 <= w[1].0, "{:?} pisa {:?}", w[0], w[1]);
    }
    let ultimo = tramos.last().unwrap();
    assert!(ultimo.0 + ultimo.1 <= TAMANO);
    for (d, _) in tramos {
        assert_eq!(d % 2, 0, "pares para el 68000");
    }
}

#[test]
fn estructuras_y_numeros_como_los_includes() {
    if !Path::new(GCC).exists() || !Path::new(NDK).exists() {
        println!("sin el compilador del 68k o el NDK: solo las comprobaciones de Rust");
        return;
    }
    let mut c = String::from(
        "#define __NO_NET_API\n#include <sys/types.h>\n#include <exec/types.h>\n#include <utility/tagitem.h>\n\
         #include <sys/socket.h>\n#include <sys/uio.h>\n#include <netinet/in.h>\n#include <netinet/tcp.h>\n\
         #include <netdb.h>\n#include <sys/filio.h>\n#include <sys/sockio.h>\n#include <sys/errno.h>\n\
         #include <libraries/bsdsocket.h>\n#include <stddef.h>\n",
    );
    let mut n = 0;
    let mut a = |expr: &str, v: i64| {
        n += 1;
        c.push_str(&format!("_Static_assert((long long)({expr}) == {v}LL, \"{}\");\n", expr.replace('"', "'")));
    };
    type Estructura<'a> = (&'a str, u32, &'a [(&'a str, u32)]);
    let estructuras: &[Estructura] = &[
        (
            "struct sockaddr_in",
            sockaddr_in::TAMANO,
            &[
                ("sin_len", sockaddr_in::SIN_LEN),
                ("sin_family", sockaddr_in::SIN_FAMILY),
                ("sin_port", sockaddr_in::SIN_PORT),
                ("sin_addr", sockaddr_in::SIN_ADDR),
                ("sin_zero", sockaddr_in::SIN_ZERO),
            ],
        ),
        (
            "struct hostent",
            hostent::TAMANO,
            &[
                ("h_name", hostent::H_NAME),
                ("h_aliases", hostent::H_ALIASES),
                ("h_addrtype", hostent::H_ADDRTYPE),
                ("h_length", hostent::H_LENGTH),
                ("h_addr_list", hostent::H_ADDR_LIST),
            ],
        ),
        (
            "struct netent",
            netent::TAMANO,
            &[
                ("n_name", netent::N_NAME),
                ("n_aliases", netent::N_ALIASES),
                ("n_addrtype", netent::N_ADDRTYPE),
                ("n_net", netent::N_NET),
            ],
        ),
        (
            "struct servent",
            servent::TAMANO,
            &[
                ("s_name", servent::S_NAME),
                ("s_aliases", servent::S_ALIASES),
                ("s_port", servent::S_PORT),
                ("s_proto", servent::S_PROTO),
            ],
        ),
        (
            "struct protoent",
            protoent::TAMANO,
            &[("p_name", protoent::P_NAME), ("p_aliases", protoent::P_ALIASES), ("p_proto", protoent::P_PROTO)],
        ),
        ("struct timeval", timeval::TAMANO, &[("tv_secs", timeval::TV_SECS), ("tv_micro", timeval::TV_MICRO)]),
        ("struct linger", linger::TAMANO, &[("l_onoff", linger::L_ONOFF), ("l_linger", linger::L_LINGER)]),
        (
            "struct msghdr",
            msghdr::TAMANO,
            &[
                ("msg_name", msghdr::MSG_NAME),
                ("msg_namelen", msghdr::MSG_NAMELEN),
                ("msg_iov", msghdr::MSG_IOV),
                ("msg_iovlen", msghdr::MSG_IOVLEN),
                ("msg_control", msghdr::MSG_CONTROL),
                ("msg_controllen", msghdr::MSG_CONTROLLEN),
                ("msg_flags", msghdr::MSG_FLAGS),
            ],
        ),
        ("struct iovec", iovec::TAMANO, &[("iov_base", iovec::IOV_BASE), ("iov_len", iovec::IOV_LEN)]),
        (
            "struct addrinfo",
            addrinfo::TAMANO,
            &[
                ("ai_flags", addrinfo::AI_FLAGS),
                ("ai_family", addrinfo::AI_FAMILY),
                ("ai_socktype", addrinfo::AI_SOCKTYPE),
                ("ai_protocol", addrinfo::AI_PROTOCOL),
                ("ai_addrlen", addrinfo::AI_ADDRLEN),
                ("ai_addr", addrinfo::AI_ADDR),
                ("ai_canonname", addrinfo::AI_CANONNAME),
                ("ai_next", addrinfo::AI_NEXT),
            ],
        ),
        ("struct TagItem", tagitem::TAMANO, &[("ti_Tag", tagitem::TI_TAG), ("ti_Data", tagitem::TI_DATA)]),
    ];
    for (s, tam, campos) in estructuras {
        a(&format!("sizeof({s})"), *tam as i64);
        for (campo, d) in *campos {
            a(&format!("offsetof({s}, {campo})"), *d as i64);
        }
    }
    a("sizeof(fd_mask)", 4);
    use errno::*;
    for (n, v) in [
        ("EINTR", EINTR),
        ("EBADF", EBADF),
        ("EFAULT", EFAULT),
        ("EINVAL", EINVAL),
        ("EMFILE", EMFILE),
        ("EPIPE", EPIPE),
        ("EWOULDBLOCK", EWOULDBLOCK),
        ("EAGAIN", EAGAIN),
        ("EINPROGRESS", EINPROGRESS),
        ("EALREADY", EALREADY),
        ("ENOTSOCK", ENOTSOCK),
        ("EDESTADDRREQ", EDESTADDRREQ),
        ("EMSGSIZE", EMSGSIZE),
        ("ENOPROTOOPT", ENOPROTOOPT),
        ("EPROTONOSUPPORT", EPROTONOSUPPORT),
        ("ESOCKTNOSUPPORT", ESOCKTNOSUPPORT),
        ("EOPNOTSUPP", EOPNOTSUPP),
        ("EAFNOSUPPORT", EAFNOSUPPORT),
        ("EADDRINUSE", EADDRINUSE),
        ("EADDRNOTAVAIL", EADDRNOTAVAIL),
        ("ENETUNREACH", ENETUNREACH),
        ("ECONNRESET", ECONNRESET),
        ("ENOBUFS", ENOBUFS),
        ("EISCONN", EISCONN),
        ("ENOTCONN", ENOTCONN),
        ("ETIMEDOUT", ETIMEDOUT),
        ("ECONNREFUSED", ECONNREFUSED),
        ("EHOSTUNREACH", EHOSTUNREACH),
        ("ENOSYS", ENOSYS),
    ] {
        a(n, v as i64);
    }
    use sock::*;
    for (n, v) in [
        ("AF_INET", AF_INET),
        ("AF_INET6", AF_INET6),
        ("SOCK_STREAM", SOCK_STREAM),
        ("SOCK_DGRAM", SOCK_DGRAM),
        ("SOCK_RAW", SOCK_RAW),
        ("IPPROTO_TCP", IPPROTO_TCP),
        ("IPPROTO_UDP", IPPROTO_UDP),
        ("SOL_SOCKET", SOL_SOCKET),
        ("SO_REUSEADDR", SO_REUSEADDR),
        ("SO_KEEPALIVE", SO_KEEPALIVE),
        ("SO_BROADCAST", SO_BROADCAST),
        ("SO_LINGER", SO_LINGER),
        ("SO_SNDBUF", SO_SNDBUF),
        ("SO_RCVBUF", SO_RCVBUF),
        ("SO_SNDTIMEO", SO_SNDTIMEO),
        ("SO_RCVTIMEO", SO_RCVTIMEO),
        ("SO_ERROR", SO_ERROR),
        ("SO_TYPE", SO_TYPE),
        ("SO_EVENTMASK", SO_EVENTMASK),
        ("TCP_NODELAY", TCP_NODELAY),
        ("IP_TTL", IP_TTL),
        ("MSG_OOB", MSG_OOB),
        ("MSG_PEEK", MSG_PEEK),
        ("MSG_WAITALL", MSG_WAITALL),
        ("MSG_DONTWAIT", MSG_DONTWAIT),
        ("SHUT_RDWR", SHUT_RDWR),
        ("INADDR_NONE", INADDR_NONE),
        ("FIONBIO", fio::FIONBIO),
        ("FIONREAD", fio::FIONREAD),
        ("FIOASYNC", fio::FIOASYNC),
        ("FIOSETOWN", fio::FIOSETOWN),
        ("FIOGETOWN", fio::FIOGETOWN),
        ("SIOCATMARK", fio::SIOCATMARK),
    ] {
        a(&format!("(unsigned long)({n})"), v as i64);
    }
    use sbt::*;
    for (n, v) in [
        ("SBTM_GETVAL(SBTC_ERRNO)", getval(SBTC_ERRNO)),
        ("SBTM_GETREF(SBTC_HERRNO)", getref(SBTC_HERRNO)),
        ("SBTM_SETVAL(SBTC_ERRNOPTR(sizeof(long)))", setval(SBTC_ERRNOLONGPTR)),
        ("SBTM_SETREF(SBTC_ERRNOPTR(sizeof(short)))", setref(SBTC_ERRNOWORDPTR)),
        ("SBTC_ERRNOPTR(sizeof(char))", SBTC_ERRNOBYTEPTR),
        ("SBTC_BREAKMASK", SBTC_BREAKMASK),
        ("SBTC_SIGIOMASK", SBTC_SIGIOMASK),
        ("SBTC_SIGEVENTMASK", SBTC_SIGEVENTMASK),
        ("SBTC_DTABLESIZE", SBTC_DTABLESIZE),
        ("SBTC_LOGTAGPTR", SBTC_LOGTAGPTR),
        ("SBTC_ERRNOSTRPTR", SBTC_ERRNOSTRPTR),
        ("SBTC_HERRNOLONGPTR", SBTC_HERRNOLONGPTR),
        ("SBTC_RELEASESTRPTR", SBTC_RELEASESTRPTR),
        ("SBTC_CAN_SHARE_LIBRARY_BASES", SBTC_CAN_SHARE_LIBRARY_BASES),
        ("SBTC_SYSTEM_STATUS", SBTC_SYSTEM_STATUS),
        ("SBTC_HAVE_ADDRESS_CONVERSION_API", SBTC_HAVE_ADDRESS_CONVERSION_API),
        ("SBTC_HAVE_GETHOSTADDR_R_API", SBTC_HAVE_GETHOSTADDR_R_API),
        ("SBTC_GET_BYTES_SENT", SBTC_GET_BYTES_SENT),
        ("UNIQUE_ID", UNIQUE_ID),
        ("FD_ACCEPT", FD_ACCEPT),
        ("FD_CONNECT", FD_CONNECT),
        ("FD_READ", FD_READ),
        ("FD_WRITE", FD_WRITE),
        ("FD_ERROR", FD_ERROR),
        ("FD_CLOSE", FD_CLOSE),
    ] {
        a(&format!("(unsigned long)({n})"), v as i64);
    }
    use netdb::*;
    for (n, v) in [
        ("HOST_NOT_FOUND", HOST_NOT_FOUND),
        ("TRY_AGAIN", TRY_AGAIN),
        ("NO_RECOVERY", NO_RECOVERY),
        ("NO_DATA", NO_DATA),
        ("NETDB_INTERNAL", NETDB_INTERNAL),
        ("EAI_NONAME", EAI_NONAME),
        ("EAI_AGAIN", EAI_AGAIN),
        ("EAI_SERVICE", EAI_SERVICE),
        ("EAI_SYSTEM", EAI_SYSTEM),
        ("EAI_FAMILY", EAI_FAMILY),
        ("EAI_MEMORY", EAI_MEMORY),
        ("AI_PASSIVE", AI_PASSIVE as i32),
        ("AI_CANONNAME", AI_CANONNAME as i32),
        ("AI_NUMERICHOST", AI_NUMERICHOST as i32),
        ("AI_NUMERICSERV", AI_NUMERICSERV as i32),
        ("NI_NUMERICHOST", NI_NUMERICHOST as i32),
        ("NI_NAMEREQD", NI_NAMEREQD as i32),
        ("NI_DGRAM", NI_DGRAM as i32),
        ("MAXHOSTNAMELEN", MAXHOSTNAMELEN as i32),
    ] {
        a(n, v as i64);
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let f = dir.join("estructuras_bsdsocket.c");
    std::fs::write(&f, &c).unwrap();
    let s = Command::new(GCC).args(["-fsyntax-only", "-I", NDK]).arg(&f).output().unwrap();
    assert!(s.status.success(), "{}", String::from_utf8_lossy(&s.stderr));
    println!("{n} aserciones cotejadas con los includes del NDK");
}
