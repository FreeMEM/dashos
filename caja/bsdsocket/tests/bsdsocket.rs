//! bsdsocket.library: bases propias, cliente TCP, no bloqueante con WaitSelect, servidor, UDP, nombres, errno,
//! dos tareas y lo que se suelta al morir.

mod comun;
use comun::*;

use caja_bsdsocket::definiciones::{fio, netdb, privado, sbt, sock};
use caja_bsdsocket::*;
use caja_exec::estructuras::{execbase, library, sig, ts};
use caja_exec::{Accion, Fallo, Memoria};

#[test]
fn cada_apertura_da_su_base() {
    let mut c = caja();
    let lib = c.s.base();
    let b1 = c.base;
    let b2 = c.abrir();
    assert_ne!(b1, b2);
    assert_ne!(b1, lib);
    // No están en LibList; la de LibList sí, con la cuenta de las dos.
    let lista = c.e.nodos(c.e.base() + execbase::LIBLIST).unwrap();
    assert!(lista.contains(&lib) && !lista.contains(&b1) && !lista.contains(&b2));
    assert_eq!(c.e.mem.leer16(lib + library::LIB_OPENCNT).unwrap(), 2);
    assert_eq!(c.e.mem.leer16(b1 + library::LIB_VERSION).unwrap(), 4);
    assert_eq!(c.texto(c.e.mem.leer32(b1 + 10).unwrap()), "bsdsocket.library");
    // Su tabla salta a las mismas funciones nativas.
    assert_eq!(c.e.entrada(b1 - 30).unwrap(), c.e.entrada(lib - 30).unwrap());
    assert_eq!(c.e.entrada(b1 - 822).unwrap(), caja_exec::Entrada::Nativa(PRIMERO + 2 + 132));
    // Descriptores y errno por base.
    let fd1 = c.zocalo(1);
    c.base = b2;
    let fd2 = c.zocalo(1);
    assert_eq!((fd1, fd2), (0, 0));
    assert_eq!(c.f("CloseSocket", &[7], &[]) as i32, -1);
    assert_eq!(c.errno(), EBADF);
    c.base = b1;
    assert_eq!(c.errno(), 0);
    c.cerrar(b2);
    c.cerrar(b1);
    assert_eq!(c.e.mem.leer16(lib + library::LIB_OPENCNT).unwrap(), 0);
    assert!(c.s.bases().is_empty());
    assert_eq!(c.s.zocalos_abiertos(), 0);
    c.sin_avisos();
    // Una versión que no hay.
    let nombre = c.cadena("bsdsocket.library");
    let mut r = caja_exec::Registros::default();
    r.a[1] = nombre;
    r.d[0] = 5;
    c.exec("OpenLibrary", &mut r);
    assert_eq!(r.d[0], 0);
}

#[test]
fn cliente_http_bloqueante() {
    let mut c = caja();
    c.red.modo = Modo::Pendiente;
    let fd = c.zocalo(1);
    let sa = c.sockaddr([93, 184, 216, 34], 80);
    // connect espera a que la red conteste.
    let r = c.bloquea("connect", &[fd, 16], &[sa]);
    assert_eq!(c.estado(c.tarea), ts::WAIT);
    assert_eq!(c.repetir("connect", &r), None, "aún no");
    let id = c.red.ultima();
    c.red.conn(id).fase = Fase::Abierta;
    assert_eq!(c.repetir("connect", &r), Some((Accion::Seguir, 0)));
    assert_eq!(c.red.conexiones_hechas, vec![([93, 184, 216, 34], 80)]);
    // La petición.
    let pet = b"GET / HTTP/1.0\r\nHost: example.com\r\n\r\n";
    let p = c.datos(pet);
    assert_eq!(c.f("send", &[fd, pet.len() as u32, 0], &[p]), pet.len() as u32);
    assert_eq!(c.red.conn(id).salida, pet);
    // La respuesta: recv espera hasta que llega algo.
    let buf = c.bloque(4096);
    let r = c.bloquea("recv", &[fd, 4096, 0], &[buf]);
    assert_eq!(c.repetir("recv", &r), None);
    c.red.llega(id, b"HTTP/1.0 200 OK\r\n\r\nhola");
    let (acc, n) = c.repetir("recv", &r).unwrap();
    assert_eq!((acc, n), (Accion::Seguir, 23));
    assert_eq!(c.leer(buf, 23), b"HTTP/1.0 200 OK\r\n\r\nhola");
    // El otro cierra: recv da 0.
    c.red.conn(id).fase = Fase::Fin;
    assert_eq!(c.f("recv", &[fd, 4096, 0], &[buf]), 0);
    assert_eq!(c.f("send", &[fd, 4, 0], &[p]) as i32, -1);
    assert_eq!(c.errno(), EPIPE);
    // getsockname y getpeername.
    let sa2 = c.bloque(16);
    let lp = c.bloque(4);
    c.e32(lp, 16);
    assert_eq!(c.f("getpeername", &[fd], &[sa2, lp]), 0);
    assert_eq!(c.leer(sa2, 8), [16, 2, 0, 80, 93, 184, 216, 34]);
    assert_eq!(c.f("getsockname", &[fd], &[sa2, lp]), 0);
    assert_eq!(&c.leer(sa2 + 4, 4), &[192, 168, 1, 50]);
    assert_eq!(c.f("CloseSocket", &[fd], &[]), 0);
    assert!(c.red.conn(id).cerrada);
    assert_eq!(c.f("CloseSocket", &[fd], &[]) as i32, -1, "dos veces");
    assert_eq!(c.errno(), EBADF);
    c.sin_avisos();
}

#[test]
fn send_grande_espera_sitio_y_conecta_rechazada() {
    let mut c = caja();
    let (fd, id) = c.conectado([10, 0, 0, 1], 25);
    c.red.conn(id).cabe = 100;
    let datos: Vec<u8> = (0..250u32).map(|i| i as u8).collect();
    let p = c.datos(&datos);
    let r = c.bloquea("send", &[fd, 250, 0], &[p]);
    assert_eq!(c.red.conn(id).salida.len(), 100);
    // La red manda: hay sitio otra vez; la repetición manda 100 más y vuelve a esperar.
    c.red.conn(id).salida.clear();
    assert!(matches!(c.repetir("send", &r), Some((Accion::Bloquear, _))));
    assert_eq!(c.red.conn(id).salida.len(), 100);
    c.red.conn(id).salida.clear();
    let (acc, n) = c.repetir("send", &r).unwrap();
    assert_eq!((acc, n), (Accion::Seguir, 250));
    assert_eq!(c.red.conn(id).salida, datos[200..]);
    // Rechazada.
    c.red.modo = Modo::Rechaza;
    let fd2 = c.zocalo(1);
    let sa = c.sockaddr([10, 0, 0, 2], 81);
    assert_eq!(c.f("connect", &[fd2, 16], &[sa]) as i32, -1);
    assert_eq!(c.errno(), ECONNREFUSED);
    // Con la conexión cortada por el otro, recv da ECONNRESET.
    c.red.conn(id).fase = Fase::Error(ErrorRed::Cortada);
    let b = c.bloque(10);
    assert_eq!(c.f("recv", &[fd, 10, 0], &[b]) as i32, -1);
    assert_eq!(c.errno(), ECONNRESET);
}

#[test]
fn no_bloqueante_con_waitselect() {
    let mut c = caja();
    c.red.modo = Modo::Pendiente;
    let fd = c.zocalo(1);
    let uno = c.datos(&1u32.to_be_bytes());
    assert_eq!(c.f("IoctlSocket", &[fd, fio::FIONBIO], &[uno]), 0);
    let sa = c.sockaddr([10, 1, 2, 3], 443);
    assert_eq!(c.f("connect", &[fd, 16], &[sa]) as i32, -1);
    assert_eq!(c.errno(), EINPROGRESS);
    assert_eq!(c.f("connect", &[fd, 16], &[sa]) as i32, -1);
    assert_eq!(c.errno(), EALREADY);
    // Sondeo: timeout de 0.
    let (lee, escribe, tv) = (c.bloque(32), c.bloque(32), c.bloque(8));
    c.e32(escribe, 1 << fd);
    assert_eq!(c.f("WaitSelect", &[fd + 1, 0], &[0, escribe, 0, tv]), 0);
    assert_eq!(c.l32(escribe), 0);
    // Conecta: se puede escribir y SO_ERROR es 0.
    let id = c.red.ultima();
    c.red.conn(id).fase = Fase::Abierta;
    c.e32(escribe, 1 << fd);
    assert_eq!(c.f("WaitSelect", &[fd + 1, 0], &[0, escribe, 0, tv]), 1);
    assert_eq!(c.l32(escribe), 1 << fd);
    let (v, l) = (c.bloque(4), c.bloque(4));
    c.e32(l, 4);
    assert_eq!(c.f("getsockopt", &[fd, sock::SOL_SOCKET, sock::SO_ERROR], &[v, l]), 0);
    assert_eq!(c.l32(v), 0);
    assert_eq!(c.f("connect", &[fd, 16], &[sa]) as i32, -1);
    assert_eq!(c.errno(), EISCONN);
    // Nada que leer: EWOULDBLOCK.
    let buf = c.bloque(64);
    assert_eq!(c.f("recv", &[fd, 64, 0], &[buf]) as i32, -1);
    assert_eq!(c.errno(), EWOULDBLOCK);
    // WaitSelect de lectura con un plazo de 2 s: espera (deja correr a las demás).
    c.e32(tv, 2);
    c.e32(lee, 1 << fd);
    let r = c.bloquea("WaitSelect", &[fd + 1, 0], &[lee, 0, 0, tv]);
    assert!(c.s.hay_esperas());
    assert_eq!(c.s.proximo(), Some(c.ahora + 2_000_000));
    c.ahora += 1_000_000;
    assert_eq!(c.repetir("WaitSelect", &r), None);
    c.red.llega(id, b"hola");
    assert_eq!(c.repetir("WaitSelect", &r), Some((Accion::Seguir, 1)));
    assert_eq!(c.l32(lee), 1 << fd);
    assert!(!c.s.hay_esperas());
    // FIONREAD.
    let n = c.bloque(4);
    assert_eq!(c.f("IoctlSocket", &[fd, fio::FIONREAD], &[n]), 0);
    assert_eq!(c.l32(n), 4);
    // MSG_PEEK no saca nada.
    assert_eq!(c.f("recv", &[fd, 64, sock::MSG_PEEK], &[buf]), 4);
    assert_eq!(c.f("recv", &[fd, 2, 0], &[buf]), 2);
    assert_eq!(c.leer(buf, 2), b"ho");
    assert_eq!(c.f("recv", &[fd, 64, 0], &[buf]), 2);
    assert_eq!(c.leer(buf, 2), b"la");
    // Vence el plazo sin nada: 0 y los conjuntos a cero.
    c.e32(lee, 1 << fd);
    c.e32(tv, 1);
    let r = c.bloquea("WaitSelect", &[fd + 1, 0], &[lee, 0, 0, tv]);
    c.ahora += 1_000_001;
    assert_eq!(c.repetir("WaitSelect", &r), Some((Accion::Seguir, 0)));
    assert_eq!(c.l32(lee), 0);
    c.sin_avisos();
}

#[test]
fn waitselect_con_senales_del_programa_y_ctrl_c() {
    let mut c = caja();
    let (fd, _) = c.conectado([10, 0, 0, 9], 7);
    let bit = c.e.alloc_signal(-1).unwrap() as u32;
    let mascara = 1 << bit;
    let lee = c.bloque(4);
    let sp = c.bloque(4);
    c.e32(lee, 1 << fd);
    c.e32(sp, mascara);
    // Sin plazo: solo las señales lo despiertan.
    let r = c.bloquea("WaitSelect", &[fd + 1, sp], &[lee, 0, 0, 0]);
    assert_eq!(c.repetir("WaitSelect", &r), None);
    c.senal(c.tarea, mascara);
    assert_eq!(c.estado(c.tarea), ts::RUN);
    // (la caja repite con los registros de entonces)
    let (acc, n) = {
        let mut rr = r;
        let k = c.s.numero("WaitSelect").unwrap();
        let a = c.s.llamar(&mut c.e, &mut c.red, k, &mut rr, c.ahora);
        (a, rr.d[0])
    };
    assert_eq!((acc, n), (Accion::Seguir, 0));
    assert_eq!(c.l32(sp), mascara, "la señal que llegó");
    assert_eq!(c.e.set_signal(0, 0).unwrap() & mascara, 0, "y se quitó de la tarea");
    // Ctrl-C: -1 y EINTR; la señal se queda puesta.
    c.e32(lee, 1 << fd);
    c.e32(sp, mascara);
    let _ = c.bloquea("WaitSelect", &[fd + 1, sp], &[lee, 0, 0, 0]);
    c.senal(c.tarea, sig::SIGBREAKF_CTRL_C);
    let (acc, n) = c.una("WaitSelect", &[fd + 1, sp], &[lee, 0, 0, 0]);
    assert_eq!((acc, n as i32), (Accion::Seguir, -1));
    assert_eq!(c.errno(), EINTR);
    assert_eq!(c.l32(lee), 1 << fd, "los conjuntos sin tocar");
    assert_ne!(c.e.set_signal(0, 0).unwrap() & sig::SIGBREAKF_CTRL_C, 0);
    // Un recv bloqueante también: EINTR al llegar Ctrl-C.
    let buf = c.bloque(16);
    let (acc, n) = c.una("recv", &[fd, 16, 0], &[buf]);
    assert_eq!((acc, n as i32), (Accion::Seguir, -1), "con Ctrl-C ya puesto no espera");
    c.e.set_signal(0, sig::SIGBREAKF_CTRL_C).unwrap();
    let r = c.bloquea("recv", &[fd, 16, 0], &[buf]);
    c.senal(c.tarea, sig::SIGBREAKF_CTRL_C);
    let (acc, n) = {
        let mut rr = r;
        let k = c.s.numero("recv").unwrap();
        let a = c.s.llamar(&mut c.e, &mut c.red, k, &mut rr, c.ahora);
        (a, rr.d[0])
    };
    assert_eq!((acc, n as i32), (Accion::Seguir, -1));
    assert_eq!(c.errno(), EINTR);
    // Sin máscara de Ctrl-C (SBTC_BREAKMASK a 0): espera aparcada y la despierta la red.
    c.e.set_signal(0, sig::SIGBREAKF_CTRL_C).unwrap();
    assert_eq!(c.etiqueta(sbt::setval(sbt::SBTC_BREAKMASK), 0).0, 0);
    let r = c.bloquea("recv", &[fd, 16, 0], &[buf]);
    assert!(c.e.aparcada(c.tarea));
    let id = c.red.ultima();
    c.red.llega(id, b"x");
    assert_eq!(c.repetir("recv", &r), Some((Accion::Seguir, 1)));
    assert!(!c.e.aparcada(c.tarea));
}

#[test]
fn servidor_bind_listen_accept() {
    let mut c = caja();
    let fd = c.zocalo(1);
    let sa = c.sockaddr([0, 0, 0, 0], 8080);
    assert_eq!(c.f("bind", &[fd, 16], &[sa]), 0);
    assert_eq!(c.f("bind", &[fd, 16], &[sa]) as i32, -1, "dos veces");
    assert_eq!(c.errno(), EINVAL);
    assert_eq!(c.f("listen", &[fd, 3], &[]), 0);
    assert_eq!(c.red.oidos_de(8080), 3);
    let (dir, lp) = (c.bloque(16), c.bloque(4));
    c.e32(lp, 16);
    let r = c.bloquea("accept", &[fd], &[dir, lp]);
    assert_eq!(c.repetir("accept", &r), None);
    let entrante = c.red.entra(8080, [10, 0, 0, 77], 51000);
    let (acc, nfd) = c.repetir("accept", &r).unwrap();
    assert_eq!((acc, nfd), (Accion::Seguir, 1));
    assert_eq!(c.leer(dir, 8), [16, 2, 0xC7, 0x38, 10, 0, 0, 77]);
    assert_eq!(c.red.oidos_de(8080), 3, "el oído gastado se repone");
    c.red.llega(entrante, b"PING");
    let buf = c.bloque(8);
    assert_eq!(c.f("recv", &[nfd, 8, 0], &[buf]), 4);
    let p = c.datos(b"PONG");
    assert_eq!(c.f("send", &[nfd, 4, 0], &[p]), 4);
    assert_eq!(c.red.conn(entrante).salida, b"PONG");
    // SO_ACCEPTCONN y un segundo zócalo en el mismo puerto.
    let (v, l) = (c.bloque(4), c.bloque(4));
    c.e32(l, 4);
    assert_eq!(c.f("getsockopt", &[fd, sock::SOL_SOCKET, sock::SO_ACCEPTCONN], &[v, l]), 0);
    assert_eq!(c.l32(v), 1);
    let otro = c.zocalo(1);
    assert_eq!(c.f("bind", &[otro, 16], &[sa]) as i32, -1);
    assert_eq!(c.errno(), EADDRINUSE);
    // Puerto 0: uno libre que getsockname dice.
    let fd3 = c.zocalo(1);
    assert_eq!(c.f("listen", &[fd3, 1], &[]), 0);
    assert_eq!(c.f("getsockname", &[fd3], &[dir, lp]), 0);
    let puerto = u16::from_be_bytes([c.leer(dir, 4)[2], c.leer(dir, 4)[3]]);
    assert!(puerto >= 49152);
    // Sin el permiso de escuchar.
    c.red.sin_permiso = true;
    let fd4 = c.zocalo(1);
    let sa4 = c.sockaddr([0; 4], 21);
    assert_eq!(c.f("bind", &[fd4, 16], &[sa4]), 0);
    assert_eq!(c.f("listen", &[fd4, 1], &[]) as i32, -1);
    assert_eq!(c.errno(), EACCES);
    // Cerrar el que escucha cierra sus oídos.
    assert_eq!(c.f("CloseSocket", &[fd], &[]), 0);
    assert_eq!(c.red.oidos_de(8080), 0);
    // accept no bloqueante sin nadie.
    let uno = c.datos(&1u32.to_be_bytes());
    c.f("IoctlSocket", &[fd3, fio::FIONBIO], &[uno]);
    assert_eq!(c.f("accept", &[fd3], &[0, 0]) as i32, -1);
    assert_eq!(c.errno(), EWOULDBLOCK);
    c.sin_avisos();
}

#[test]
fn udp_sendto_recvfrom_connect() {
    let mut c = caja();
    let fd = c.zocalo(2);
    let dns = c.sockaddr([8, 8, 8, 8], 53);
    let q = c.datos(b"pregunta");
    assert_eq!(c.f("send", &[fd, 8, 0], &[q]) as i32, -1);
    assert_eq!(c.errno(), EDESTADDRREQ);
    assert_eq!(c.f("sendto", &[fd, 8, 0, 16], &[q, dns]), 8);
    let u = *c.red.udps.keys().next().unwrap();
    assert_eq!(c.red.udps[&u].salida, vec![(b"pregunta".to_vec(), [8, 8, 8, 8], 53)]);
    // La respuesta, con quién la manda; un búfer pequeño la recorta.
    c.red.udps.get_mut(&u).unwrap().entrada.push_back((b"respuesta".to_vec(), [8, 8, 8, 8], 53));
    let (buf, de, lp) = (c.bloque(64), c.bloque(16), c.bloque(4));
    c.e32(lp, 16);
    assert_eq!(c.f("recvfrom", &[fd, 64, sock::MSG_PEEK], &[buf, de, lp]), 9);
    assert_eq!(c.f("recvfrom", &[fd, 4, 0], &[buf, de, lp]), 4);
    assert_eq!(c.leer(buf, 4), b"resp");
    assert_eq!(c.leer(de, 8), [16, 2, 0, 53, 8, 8, 8, 8]);
    // Conectado: lo de otros se tira.
    assert_eq!(c.f("connect", &[fd, 16], &[dns]), 0);
    c.red.udps.get_mut(&u).unwrap().entrada.push_back((b"intruso".to_vec(), [6, 6, 6, 6], 53));
    c.red.udps.get_mut(&u).unwrap().entrada.push_back((b"bueno".to_vec(), [8, 8, 8, 8], 53));
    assert_eq!(c.f("recv", &[fd, 64, 0], &[buf]), 5);
    assert_eq!(c.leer(buf, 5), b"bueno");
    // recv espera al siguiente.
    let r = c.bloquea("recv", &[fd, 64, 0], &[buf]);
    c.red.udps.get_mut(&u).unwrap().entrada.push_back((b"otra".to_vec(), [8, 8, 8, 8], 53));
    assert_eq!(c.repetir("recv", &r), Some((Accion::Seguir, 4)));
    // Demasiado grande.
    let g = c.bloque(2000);
    assert_eq!(c.f("send", &[fd, 2000, 0], &[g]) as i32, -1);
    assert_eq!(c.errno(), EMSGSIZE);
    // bind a un puerto en UDP lo abre allí.
    let fd2 = c.zocalo(2);
    let sa = c.sockaddr([0; 4], 5353);
    assert_eq!(c.f("bind", &[fd2, 16], &[sa]), 0);
    assert!(c.red.udps.values().any(|x| x.puerto == 5353));
    c.sin_avisos();
}

#[test]
fn gethostbyname_y_nombres() {
    let mut c = caja();
    let n = c.cadena("1.2.3.4");
    let h = c.f("gethostbyname", &[], &[n]);
    assert_eq!(h, c.base + privado::HOSTENT, "la hostent de la base");
    let lista = c.l32(h + 16);
    assert_eq!(c.leer(c.l32(lista), 4), [1, 2, 3, 4]);
    assert_eq!(c.l32(lista + 4), 0);
    assert_eq!(c.l32(h + 8), 2);
    assert_eq!(c.l32(h + 12), 4);
    assert_eq!(c.texto(c.l32(h)), "1.2.3.4");
    assert_eq!(c.l32(c.l32(h + 4)), 0, "sin alias");
    // Por el servicio: espera.
    c.red.dns.insert(b"example.com".to_vec(), Ok(vec![[93, 184, 216, 34], [93, 184, 216, 35]]));
    let n = c.cadena("example.com");
    let r = c.bloquea("gethostbyname", &[], &[n]);
    assert_eq!(c.repetir("gethostbyname", &r), None);
    c.red.contestar_dns();
    let (acc, h2) = c.repetir("gethostbyname", &r).unwrap();
    assert_eq!((acc, h2), (Accion::Seguir, h), "la misma memoria cada vez");
    let lista = c.l32(h + 16);
    assert_eq!(c.leer(c.l32(lista + 4), 4), [93, 184, 216, 35]);
    assert_eq!(c.texto(c.l32(h)), "example.com");
    // Un nombre que no existe: nulo y h_errno.
    let n = c.cadena("no.existe");
    let r = c.bloquea("gethostbyname", &[], &[n]);
    c.red.contestar_dns();
    assert_eq!(c.repetir("gethostbyname", &r), Some((Accion::Seguir, 0)));
    assert_eq!(c.etiqueta(sbt::getval(sbt::SBTC_HERRNO), 0).1, netdb::HOST_NOT_FOUND as u32);
    // localhost sin preguntar.
    let n = c.cadena("localhost");
    let h = c.f("gethostbyname", &[], &[n]);
    assert_eq!(c.leer(c.l32(c.l32(h + 16)), 4), [127, 0, 0, 1]);
    // gethostbyaddr: solo localhost y la propia.
    let a = c.datos(&[127, 0, 0, 1]);
    let h = c.f("gethostbyaddr", &[4, 2], &[a]);
    assert_eq!(c.texto(c.l32(h)), "localhost");
    let a = c.datos(&[1, 1, 1, 1]);
    assert_eq!(c.f("gethostbyaddr", &[4, 2], &[a]), 0);
    // gethostbyname_r en un búfer del programa, y uno que no cabe.
    let (hp, buf, he) = (c.bloque(20), c.bloque(64), c.bloque(4));
    let n = c.cadena("1.2.3.4");
    assert_eq!(c.f("gethostbyname_r", &[64], &[n, hp, buf, he]), hp);
    assert_eq!(c.leer(c.l32(c.l32(hp + 16)), 4), [1, 2, 3, 4]);
    assert_eq!(c.f("gethostbyname_r", &[10], &[n, hp, buf, he]), 0);
    assert_eq!(c.errno(), ERANGE);
    // Inet_NtoA, inet_addr, inet_network, clases.
    let p = c.f("Inet_NtoA", &[0xC0A8_0101], &[]);
    assert_eq!(c.texto(p), "192.168.1.1");
    assert_eq!(p, c.base + privado::NTOA);
    for (t, v) in
        [("10.1.2.3", 0x0A01_0203u32), ("10.258", 0x0A00_0102), ("0x7f.1", 0x7F00_0001), ("010.0.0.1", 0x0800_0001)]
    {
        let s = c.cadena(t);
        assert_eq!(c.f("inet_addr", &[], &[s]), v, "{t}");
    }
    let s = c.cadena("1.2.3.4.5");
    assert_eq!(c.f("inet_addr", &[], &[s]), 0xFFFF_FFFF);
    let s = c.cadena("10.1");
    assert_eq!(c.f("inet_network", &[], &[s]), 0x0A01);
    assert_eq!(c.f("Inet_NetOf", &[0x0A01_0203], &[]), 10);
    assert_eq!(c.f("Inet_LnaOf", &[0xC0A8_0105], &[]), 5);
    assert_eq!(c.f("Inet_MakeAddr", &[10, 0x010203], &[]), 0x0A01_0203);
    // inet_aton, inet_pton, inet_ntop.
    let (s, d) = (c.cadena("172.16.0.1"), c.bloque(4));
    assert_eq!(c.f("inet_aton", &[], &[s, d]), 1);
    assert_eq!(c.l32(d), 0xAC10_0001);
    let s2 = c.cadena("172.016.0.1");
    assert_eq!(c.f("inet_pton", &[2], &[s2, d]), 0, "pton no admite ceros delante");
    assert_eq!(c.f("inet_pton", &[2], &[s, d]), 1);
    assert_eq!(c.f("inet_pton", &[23], &[s, d]) as i32, -1);
    let t = c.bloque(16);
    assert_eq!(c.f("inet_ntop", &[2, 16], &[d, t]), t);
    assert_eq!(c.texto(t), "172.16.0.1");
    assert_eq!(c.f("inet_ntop", &[2, 5], &[d, t]), 0);
    assert_eq!(c.errno(), ENOSPC);
    // Servicios, protocolos, redes.
    let (h, tcp) = (c.cadena("https"), c.cadena("tcp"));
    let se = c.f("getservbyname", &[], &[h, tcp]);
    assert_eq!(c.l32(se + 8), 443);
    assert_eq!(c.texto(c.l32(se + 12)), "tcp");
    let se = c.f("getservbyport", &[110], &[0]);
    assert_eq!(c.texto(c.l32(se)), "pop3");
    let d = c.cadena("dns");
    let se = c.f("getservbyname", &[], &[d, 0]);
    assert_eq!(c.l32(se + 8), 53);
    let x = c.cadena("nada");
    assert_eq!(c.f("getservbyname", &[], &[x, 0]), 0);
    let u = c.cadena("udp");
    let p = c.f("getprotobyname", &[], &[u]);
    assert_eq!(c.l32(p + 8), 17);
    let p = c.f("getprotobynumber", &[6], &[]);
    assert_eq!(c.texto(c.l32(p)), "tcp");
    let l = c.cadena("loopback");
    let ne = c.f("getnetbyname", &[], &[l]);
    assert_eq!(c.l32(ne + 12), 127);
    // Los recorridos.
    c.f("setservent", &[1], &[]);
    let mut k = 0;
    while c.f("getservent", &[], &[]) != 0 {
        k += 1;
    }
    assert_eq!(k, nombres::SERVICIOS.len());
    // gethostname y gethostid.
    let b = c.bloque(32);
    assert_eq!(c.f("gethostname", &[32], &[b]), 0);
    assert_eq!(c.texto(b), "amiga");
    assert_eq!(c.f("gethostid", &[], &[]), 0xC0A8_0132);
    c.sin_avisos();
}

#[test]
fn getaddrinfo_freeaddrinfo_getnameinfo() {
    let mut c = caja();
    let res = c.bloque(4);
    let (h, s) = (c.cadena("10.0.0.5"), c.cadena("http"));
    assert_eq!(c.f("getaddrinfo", &[], &[h, s, 0, res]), 0);
    let ai = c.l32(res);
    // Solo TCP (http no está en UDP).
    assert_eq!(c.l32(ai + 8), 1);
    assert_eq!(c.l32(ai + 28), 0);
    let sa = c.l32(ai + 20);
    assert_eq!(c.leer(sa, 8), [16, 2, 0, 80, 10, 0, 0, 5]);
    c.f("freeaddrinfo", &[], &[ai]);
    assert!(c.e.bloque(ai).is_none(), "suelto");
    // Por nombre, con pistas: UDP, puerto en número, nombre canónico.
    c.red.dns.insert(b"ntp.org".to_vec(), Ok(vec![[1, 2, 3, 4], [5, 6, 7, 8]]));
    let pistas = c.bloque(32);
    c.e32(pistas, netdb::AI_CANONNAME);
    c.e32(pistas + 8, 2);
    let (h, s) = (c.cadena("ntp.org"), c.cadena("123"));
    let r = c.bloquea("getaddrinfo", &[], &[h, s, pistas, res]);
    c.red.contestar_dns();
    assert_eq!(c.repetir("getaddrinfo", &r), Some((Accion::Seguir, 0)));
    let ai = c.l32(res);
    assert_eq!(c.texto(c.l32(ai + 24)), "ntp.org");
    let ai2 = c.l32(ai + 28);
    assert_eq!(c.leer(c.l32(ai2 + 20) + 2, 6), [0, 123, 5, 6, 7, 8]);
    assert_eq!(c.l32(ai2 + 28), 0);
    c.f("freeaddrinfo", &[], &[ai]);
    // Errores.
    let (h, s) = (c.cadena("no.hay"), c.cadena("80"));
    let r = c.bloquea("getaddrinfo", &[], &[h, s, 0, res]);
    c.red.contestar_dns();
    assert_eq!(c.repetir("getaddrinfo", &r), Some((Accion::Seguir, netdb::EAI_NONAME as u32)));
    let s = c.cadena("noexiste");
    assert_eq!(c.f("getaddrinfo", &[], &[0, s, 0, res]) as i32, netdb::EAI_SERVICE);
    assert_eq!(c.f("getaddrinfo", &[], &[0, 0, 0, res]) as i32, netdb::EAI_NONAME);
    let t = c.f("gai_strerror", &[netdb::EAI_NONAME as u32], &[]);
    assert_eq!(c.texto(t), "Name or service is not known");
    let t = c.f("gai_strerror", &[77], &[]);
    assert_eq!(c.texto(t), "Unknown error");
    // getnameinfo.
    let sa = c.sockaddr([127, 0, 0, 1], 443);
    let (hb, sb) = (c.bloque(64), c.bloque(16));
    assert_eq!(c.f("getnameinfo", &[16, 64, 16, 0], &[sa, hb, sb]), 0);
    assert_eq!((c.texto(hb), c.texto(sb)), ("localhost".into(), "https".into()));
    assert_eq!(c.f("getnameinfo", &[16, 64, 16, netdb::NI_NUMERICHOST | netdb::NI_NUMERICSERV], &[sa, hb, sb]), 0);
    assert_eq!((c.texto(hb), c.texto(sb)), ("127.0.0.1".into(), "443".into()));
    // freeaddrinfo de algo que no dio getaddrinfo: aviso, nada roto.
    let x = c.bloque(32);
    c.f("freeaddrinfo", &[], &[x]);
    assert_eq!(c.avisos_de("freeaddrinfo"), 1);
    assert!(c.e.bloque(x).is_some());
}

#[test]
fn errno_por_puntero_de_1_2_y_4_bytes() {
    let mut c = caja();
    let p = c.bloque(8);
    for (codigo, tam) in [(sbt::SBTC_ERRNOBYTEPTR, 1u32), (sbt::SBTC_ERRNOWORDPTR, 2), (sbt::SBTC_ERRNOLONGPTR, 4)] {
        c.e.mem.leer_mut(p, 8).unwrap().fill(0xAA);
        assert_eq!(c.etiqueta(sbt::setval(codigo), p).0, 0);
        assert_eq!(c.f("CloseSocket", &[33], &[]) as i32, -1);
        let v = c.leer(p, 8);
        match tam {
            1 => assert_eq!(v[..2], [EBADF as u8, 0xAA]),
            2 => assert_eq!(v[..3], [0, EBADF as u8, 0xAA]),
            _ => assert_eq!(v[..5], [0, 0, 0, EBADF as u8, 0xAA]),
        }
        assert_eq!(c.errno(), EBADF);
    }
    // SetErrnoPtr (la antigua) y SBTC_ERRNO.
    let q = c.bloque(4);
    c.f("SetErrnoPtr", &[2], &[q]);
    c.f("socket", &[99, 1, 0], &[]);
    assert_eq!(c.e.mem.leer16(q).unwrap() as i32, EAFNOSUPPORT);
    assert_eq!(c.etiqueta(ERRNO_GET, 0).1 as i32, EAFNOSUPPORT);
    assert_eq!(c.etiqueta(sbt::setval(sbt::SBTC_ERRNO), 5).0, 0);
    assert_eq!(c.e.mem.leer16(q).unwrap(), 5);
    // h_errno por puntero.
    let h = c.bloque(4);
    assert_eq!(c.etiqueta(sbt::setval(sbt::SBTC_HERRNOLONGPTR), h).0, 0);
    let a = c.datos(&[1, 1, 1, 1]);
    c.f("gethostbyaddr", &[4, 2], &[a]);
    assert_eq!(c.l32(h), netdb::HOST_NOT_FOUND as u32);
    c.sin_avisos();
}

#[test]
fn socket_base_tag_list() {
    let mut c = caja();
    assert_eq!(c.etiqueta(sbt::getval(sbt::SBTC_BREAKMASK), 0), (0, sig::SIGBREAKF_CTRL_C));
    assert_eq!(c.etiqueta(sbt::getval(sbt::SBTC_DTABLESIZE), 0), (0, 64));
    assert_eq!(c.f("getdtablesize", &[], &[]), 64);
    let t = c.etiqueta(sbt::getval(sbt::SBTC_RELEASESTRPTR), 0).1;
    assert!(c.texto(t).starts_with("bsdsocket.library 4"));
    // El texto de un errno: entra el código, sale el puntero.
    let t = c.etiqueta(sbt::getval(sbt::SBTC_ERRNOSTRPTR), ECONNREFUSED as u32).1;
    assert_eq!(c.texto(t), "Connection refused");
    let t = c.etiqueta(sbt::getval(sbt::SBTC_HERRNOSTRPTR), 1).1;
    assert_eq!(c.texto(t), "Unknown host");
    // Por referencia.
    let v = c.bloque(4);
    assert_eq!(c.etiqueta(sbt::getref(sbt::SBTC_SYSTEM_STATUS), v).0, 0);
    assert_ne!(c.l32(v), 0);
    c.e32(v, 0x100);
    assert_eq!(c.etiqueta(sbt::setref(sbt::SBTC_SIGEVENTMASK), v).0, 0);
    assert_eq!(c.etiqueta(sbt::getval(sbt::SBTC_SIGEVENTMASK), 0).1, 0x100);
    assert_eq!(c.etiqueta(sbt::getval(sbt::SBTC_HAVE_ADDRESS_CONVERSION_API), 0).1, 1);
    assert_eq!(c.etiqueta(sbt::getval(sbt::SBTC_HAVE_ROUTING_API), 0).1, 0);
    // Varias, y la que no se sabe da su número (desde 1).
    let t = c.bloque(40);
    for (i, (tag, dato)) in [
        (sbt::setval(sbt::SBTC_LOGTAGPTR), 0),
        (1u32, 0), // TAG_IGNORE no cuenta
        (sbt::setval(sbt::SBTC_LOGMASK), 0xFF),
        (sbt::setval(sbt::SBTC_FDCALLBACK), 0),
        (0, 0),
    ]
    .iter()
    .enumerate()
    {
        c.e32(t + 8 * i as u32, *tag);
        c.e32(t + 8 * i as u32 + 4, *dato);
    }
    assert_eq!(c.f("SocketBaseTagList", &[], &[t]), 3);
    assert_eq!(c.avisos_de("SocketBaseTagList"), 1);
    // DTABLESIZE más grande, y no por debajo de lo abierto.
    assert_eq!(c.etiqueta(sbt::setval(sbt::SBTC_DTABLESIZE), 256).0, 0);
    assert_eq!(c.f("getdtablesize", &[], &[]), 256);
    // syslog.
    c.etiqueta(sbt::setval(sbt::SBTC_LOGTAGPTR), 0);
    let (m, s) = (c.cadena("conecto a %s:%d"), c.cadena("example.com"));
    let args = c.bloque(8);
    c.e32(args, s);
    c.e32(args + 4, 80);
    c.f("vsyslog", &[6], &[m, args]);
    assert_eq!(c.s.tomar_registro(), vec!["conecto a example.com:80".to_string()]);
}

#[test]
fn dos_tareas_con_sus_bases() {
    let mut c = caja();
    let t1 = c.tarea;
    let b1 = c.base;
    let t2 = c.e.crear_tarea("otra", 0, 4096, 0x10_0000).unwrap();
    // t1 espera en recv; corre t2.
    let (fd, id) = c.conectado([10, 0, 0, 1], 80);
    let buf = c.bloque(16);
    let r = c.bloquea("recv", &[fd, 16, 0], &[buf]);
    assert_eq!(c.e.elegir_siguiente(), Some(t2));
    assert_eq!(c.e.tarea_actual(), t2);
    let b2 = c.abrir();
    assert_ne!(b2, b1);
    assert_eq!(c.s.tarea_de(b2), Some(t2));
    c.base = b2;
    let (fd2, id2) = c.conectado([10, 0, 0, 2], 80);
    assert_eq!(fd2, 0, "sus descriptores empiezan en 0");
    c.f("CloseSocket", &[9], &[]);
    assert_eq!(c.errno(), EBADF);
    // Llega lo de t1: atender la despierta, aunque corre t2.
    c.red.llega(id, b"para t1");
    assert_eq!(c.s.atender(&mut c.e, &mut c.red, c.ahora), 1);
    assert_eq!(c.estado(t1), ts::READY);
    // t2 espera; vuelve t1 y repite.
    let buf2 = c.bloque(16);
    let r2 = c.bloquea("recv", &[fd2, 16, 0], &[buf2]);
    assert_eq!(c.e.elegir_siguiente(), Some(t1));
    c.base = b1;
    let mut rr = r;
    let k = c.s.numero("recv").unwrap();
    assert_eq!(c.s.llamar(&mut c.e, &mut c.red, k, &mut rr, c.ahora), Accion::Seguir);
    assert_eq!(rr.d[0], 7);
    assert_eq!(c.errno(), 0, "el EBADF fue de la otra base");
    assert_eq!(c.s.bloqueadas(), vec![t2]);
    // t2 muere esperando: su base y sus zócalos se van.
    c.s.tarea_muerta(&mut c.e, &mut c.red, t2);
    c.e.matar_tarea(t2).unwrap();
    assert!(c.red.conn(id2).cerrada);
    assert!(!c.s.bases().contains(&b2));
    assert!(c.s.bloqueadas().is_empty());
    let _ = r2;
    assert!(c.e.avisos().is_empty(), "{:?}", c.e.avisos());
}

#[test]
fn tarea_que_muere_con_zocalos_abiertos() {
    let mut c = caja();
    let t2 = c.e.crear_tarea("hija", 5, 4096, 0x10_0000).unwrap();
    assert_eq!(c.e.elegir_siguiente(), Some(t2));
    let b = c.abrir();
    c.base = b;
    let (_, id) = c.conectado([1, 1, 1, 1], 80);
    let fd = c.zocalo(1);
    let sa = c.sockaddr([0; 4], 9000);
    c.f("bind", &[fd, 16], &[sa]);
    c.f("listen", &[fd, 2], &[]);
    let u = c.zocalo(2);
    let d = c.sockaddr([9, 9, 9, 9], 9);
    let p = c.datos(b"x");
    c.f("sendto", &[u, 1, 0, 16], &[p, d]);
    let n = c.cadena("lento.org");
    let res = c.bloque(4);
    let _ = c.bloquea("getaddrinfo", &[], &[n, 0, 0, res]);
    assert_eq!(c.s.zocalos_abiertos(), 3);
    let lib = c.s.base();
    assert_eq!(c.e.mem.leer16(lib + library::LIB_OPENCNT).unwrap(), 2);
    c.s.tarea_muerta(&mut c.e, &mut c.red, t2);
    c.e.matar_tarea(t2).unwrap();
    assert!(c.red.conn(id).cerrada);
    assert_eq!(c.red.oidos_de(9000), 0);
    assert!(c.red.udps.values().all(|x| x.cerrado));
    assert_eq!(c.red.consultas_cerradas.len(), 1);
    assert_eq!(c.s.zocalos_abiertos(), 0);
    assert_eq!(c.e.mem.leer16(lib + library::LIB_OPENCNT).unwrap(), 1);
    assert!(c.e.avisos().is_empty(), "{:?}", c.e.avisos());
    assert!(c.s.avisos().is_empty(), "{:?}", c.s.avisos());
}

#[test]
fn dup2_release_y_obtain() {
    let mut c = caja();
    let (fd, id) = c.conectado([10, 0, 0, 1], 80);
    assert_eq!(c.f("Dup2Socket", &[fd, u32::MAX], &[]), 1);
    assert_eq!(c.f("Dup2Socket", &[fd, 5], &[]), 5);
    c.f("CloseSocket", &[fd], &[]);
    c.f("CloseSocket", &[1], &[]);
    assert!(!c.red.conn(id).cerrada, "queda el 5");
    // A otra base por la lista pública.
    let b1 = c.base;
    let ident = c.f("ReleaseSocket", &[5, sbt::UNIQUE_ID], &[]);
    assert!(ident > 0xFFFF);
    assert_eq!(c.f("CloseSocket", &[5], &[]) as i32, -1, "ya no es de esta base");
    let b2 = c.abrir();
    c.base = b2;
    assert_eq!(c.f("ObtainSocket", &[ident, 2, 1, 0], &[]), 0);
    assert_eq!(c.f("ObtainSocket", &[ident, 2, 1, 0], &[]) as i32, -1);
    assert_eq!(c.errno(), EBADF);
    let p = c.datos(b"ok");
    assert_eq!(c.f("send", &[0, 2, 0], &[p]), 2);
    // Una copia: los dos lo tienen.
    assert_eq!(c.f("ReleaseCopyOfSocket", &[0, 1234], &[]), 1234);
    c.base = b1;
    assert_eq!(c.f("ObtainSocket", &[1234, 2, 2, 0], &[]) as i32, -1, "otro tipo");
    assert_eq!(c.f("ObtainSocket", &[1234, 2, 1, 6], &[]), 0);
    c.base = b2;
    c.cerrar(b2);
    assert!(!c.red.conn(id).cerrada);
    c.base = b1;
    c.f("CloseSocket", &[0], &[]);
    assert!(c.red.conn(id).cerrada);
    c.sin_avisos();
}

#[test]
fn eventos_asincronos_y_get_socket_events() {
    let mut c = caja();
    let (fd, id) = c.conectado([10, 0, 0, 1], 80);
    let bit = c.e.alloc_signal(-1).unwrap() as u32;
    c.etiqueta(sbt::setval(sbt::SBTC_SIGEVENTMASK), 1 << bit);
    let m = c.datos(&(sbt::FD_READ | sbt::FD_CLOSE).to_be_bytes());
    assert_eq!(c.f("setsockopt", &[fd, sock::SOL_SOCKET, sock::SO_EVENTMASK, 4], &[m]), 0);
    let ev = c.bloque(4);
    assert_eq!(c.f("GetSocketEvents", &[], &[ev]) as i32, -1);
    c.red.llega(id, b"algo");
    c.s.atender(&mut c.e, &mut c.red, c.ahora);
    assert_ne!(c.e.set_signal(0, 0).unwrap() & (1 << bit), 0, "la señal de SBTC_SIGEVENTMASK");
    assert_eq!(c.f("GetSocketEvents", &[], &[ev]), fd);
    assert_eq!(c.l32(ev), sbt::FD_READ);
    assert_eq!(c.f("GetSocketEvents", &[], &[ev]) as i32, -1, "recogido");
    c.red.conn(id).fase = Fase::Fin;
    assert_eq!(c.f("GetSocketEvents", &[], &[ev]), fd);
    assert_eq!(c.l32(ev), sbt::FD_CLOSE);
    // FIOASYNC: la de SBTC_SIGIOMASK.
    let bit2 = c.e.alloc_signal(-1).unwrap() as u32;
    c.etiqueta(sbt::setval(sbt::SBTC_SIGIOMASK), 1 << bit2);
    let (fd2, id2) = c.conectado([10, 0, 0, 2], 80);
    let uno = c.datos(&1u32.to_be_bytes());
    c.f("IoctlSocket", &[fd2, fio::FIOASYNC], &[uno]);
    c.s.atender(&mut c.e, &mut c.red, c.ahora);
    c.e.set_signal(0, 1 << bit2).unwrap();
    c.red.llega(id2, b"z");
    c.s.atender(&mut c.e, &mut c.red, c.ahora);
    assert_ne!(c.e.set_signal(0, 0).unwrap() & (1 << bit2), 0);
}

#[test]
fn opciones_y_shutdown() {
    let mut c = caja();
    let (fd, _) = c.conectado([10, 0, 0, 1], 80);
    let (v, l) = (c.bloque(8), c.bloque(4));
    c.e32(v, 1);
    assert_eq!(c.f("setsockopt", &[fd, sock::IPPROTO_TCP, sock::TCP_NODELAY, 4], &[v]), 0);
    c.e32(l, 4);
    c.e32(v, 0);
    assert_eq!(c.f("getsockopt", &[fd, sock::IPPROTO_TCP, sock::TCP_NODELAY], &[v, l]), 0);
    assert_eq!(c.l32(v), 1);
    assert_eq!(c.f("getsockopt", &[fd, sock::SOL_SOCKET, sock::SO_TYPE], &[v, l]), 0);
    assert_eq!(c.l32(v), 1);
    assert_eq!(c.f("getsockopt", &[fd, sock::SOL_SOCKET, 0x7777], &[v, l]) as i32, -1);
    assert_eq!(c.errno(), ENOPROTOOPT);
    // SO_RCVTIMEO: recv vence con EWOULDBLOCK.
    c.e32(v, 0);
    c.e32(v + 4, 500_000);
    assert_eq!(c.f("setsockopt", &[fd, sock::SOL_SOCKET, sock::SO_RCVTIMEO, 8], &[v]), 0);
    let buf = c.bloque(8);
    let r = c.bloquea("recv", &[fd, 8, 0], &[buf]);
    c.ahora += 600_000;
    assert_eq!(c.repetir("recv", &r), Some((Accion::Seguir, u32::MAX)));
    assert_eq!(c.errno(), EWOULDBLOCK);
    c.e32(l, 8);
    assert_eq!(c.f("getsockopt", &[fd, sock::SOL_SOCKET, sock::SO_RCVTIMEO], &[v, l]), 0);
    assert_eq!((c.l32(v), c.l32(v + 4)), (0, 500_000));
    // shutdown.
    assert_eq!(c.f("shutdown", &[fd, 1], &[]), 0);
    let p = c.datos(b"x");
    assert_eq!(c.f("send", &[fd, 1, 0], &[p]) as i32, -1);
    assert_eq!(c.errno(), EPIPE);
    assert_eq!(c.f("shutdown", &[fd, 9], &[]) as i32, -1);
    let fd2 = c.zocalo(1);
    assert_eq!(c.f("shutdown", &[fd2, 2], &[]) as i32, -1);
    assert_eq!(c.errno(), ENOTCONN);
    // Una IoctlSocket que no se sabe: EINVAL y aviso.
    assert_eq!(c.f("IoctlSocket", &[fd, 0xC020_6924], &[v]) as i32, -1);
    assert_eq!(c.errno(), EINVAL);
    assert_eq!(c.avisos_de("IoctlSocket"), 1);
}

#[test]
fn lo_no_hecho_avisa_y_no_cuelga() {
    let mut c = caja();
    assert_eq!(c.f("bpf_open", &[0], &[]) as i32, -1);
    assert_eq!(c.errno(), EOPNOTSUPP);
    assert_eq!(c.f("GetRouteInfo", &[2, 0], &[]), 0);
    assert_eq!(c.f("ObtainInterfaceList", &[], &[]), 0);
    assert_eq!(c.f("(reservada)", &[], &[]) as i32, -1);
    assert!(c.s.avisos().iter().any(|a| matches!(a.fallo, Fallo::NoImplementada { .. })));
    // socket de tipos que no hay.
    assert_eq!(c.f("socket", &[2, 3, 1], &[]) as i32, -1);
    assert_eq!(c.errno(), ESOCKTNOSUPPORT);
    assert_eq!(c.f("socket", &[2, 1, 17], &[]) as i32, -1);
    assert_eq!(c.errno(), EPROTONOSUPPORT);
    assert_eq!(c.f("socket", &[23, 1, 0], &[]) as i32, -1);
    assert_eq!(c.errno(), EAFNOSUPPORT);
}

/// Cuánto tarda `atender`: con 64 tareas esperando en WaitSelect de 4 zócalos cada una, sin nada listo.
#[test]
fn coste_de_atender() {
    let mut c = caja();
    for _ in 0..64 {
        c.base = c.abrir();
        let mut fds = 0u32;
        for _ in 0..4 {
            let (fd, _) = c.conectado([10, 0, 0, 1], 80);
            fds |= 1 << fd;
        }
        let set = c.bloque(4);
        c.e32(set, fds);
        let _ = c.bloquea("WaitSelect", &[4, 0], &[set, 0, 0, 0]);
        let t = c.e.crear_tarea("t", 0, 2048, 0x10_0000).unwrap();
        assert_eq!(c.e.elegir_siguiente(), Some(t));
    }
    assert_eq!(c.s.bloqueadas().len(), 64);
    let inicio = std::time::Instant::now();
    let vueltas = 1000;
    for _ in 0..vueltas {
        assert_eq!(c.s.atender(&mut c.e, &mut c.red, c.ahora), 0);
    }
    let us = inicio.elapsed().as_secs_f64() * 1e6 / vueltas as f64;
    println!("atender: {:.1} µs con 64 tareas en WaitSelect de 4 zócalos (sin optimizar)", us);
    // Sin nadie esperando: nada.
    let mut d = caja();
    let inicio = std::time::Instant::now();
    for _ in 0..vueltas {
        d.s.atender(&mut d.e, &mut d.red, d.ahora);
    }
    println!("atender vacío: {:.3} µs", inicio.elapsed().as_secs_f64() * 1e6 / vueltas as f64);
}
