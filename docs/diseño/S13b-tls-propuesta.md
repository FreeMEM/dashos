# S13b. TLS 1.3 de cliente: propuesta para decidir

> **2026-11-15, rama `red-dns`.** No hay nada implementado. Esto es el estudio que pedía el encargo: qué hace falta
> para tener TLS 1.3 de cliente sin `std`, cuánto ocupa, de qué depende, dónde vivirían las raíces de confianza y cuánto
> costaría. La decisión es del usuario.

## Lo que se midió

Un programa de DashOS de prueba (fuera del repositorio, en un directorio temporal) con **rustls 0.23.46** sin `std`
(`default-features = false`, API `UnbufferedClientConnection`, solo TLS 1.3), el proveedor de criptografía
**rustls-rustcrypto 0.0.2-alpha** (todo en Rust) y las raíces de Mozilla (**webpki-roots 1.0.12**). Compilado para
`aarch64-unknown-none-softfloat` como los demás programas de `usuario/` (nightly, `build-std`), con `opt-level = "s"` y
LTO. El programa crea la configuración, la conexión y saca el primer mensaje (ClientHello). **No se ha ejecutado en
DashOS ni ha hablado con ningún servidor**: mide que compila, que enlaza y cuánto ocupa.

| Medida | Valor |
|---|---|
| Código (`.text`) | 546 KB |
| Datos de solo lectura (`.rodata`) con las raíces de Mozilla | 113 KB |
| Datos de solo lectura sin las raíces | 55 KB (las raíces son unos 58 KB) |
| Total en memoria | 664 KB (712 KB el fichero) |
| Crates que entran | 74 |

Lo que hubo que darle: el montón (`dashos::monton!`), el azar (`getrandom` con la opción `custom`, apuntando a
`dashos::azar`) y la hora (`rustls::time_provider::TimeProvider`, con `dashos::hora_ns`). Sin hora buena no se pueden
validar certificados: por eso este trabajo va detrás del de la hora por la red (`hora`, S13).

Licencias de lo que entra: MIT o Apache-2.0 casi todo; BSD-3-Clause (`curve25519-dalek`, `ed25519-dalek`,
`x25519-dalek`, `subtle`); ISC (`rustls-webpki`, `untrusted`; rustls es Apache-2.0, ISC o MIT); y las raíces,
CDLA-Permissive-2.0 (son datos de Mozilla). Todas compatibles con MPL-2.0. Ninguna GPL.

## Las opciones

### A. rustls sin `std` con un proveedor en Rust (lo medido)

- Gana: es la pila que recomienda S13, la más revisada de las de Rust; el protocolo (la máquina de estados, los
  mensajes, la validación de certificados con `rustls-webpki`) está auditado. La API sin búferes encaja con los anillos
  del servicio de red: el programa mueve bytes y rustls no toca la red. Trae ChaCha20-Poly1305 (lo que pide S13 para
  la Pi 4, sin AES por hardware) y AES-GCM. RSA, ECDSA P-256 y P-384 y Ed25519 para las firmas.
- Cuesta: **el proveedor `rustls-rustcrypto` no está auditado como conjunto** y su README dice, en mayúsculas, que no
  se use en producción (la 0.1.0-pre.0, para rustls 0.23, lo sigue diciendo). Las primitivas de RustCrypto sí tienen
  auditorías sueltas (ChaCha20-Poly1305 y AES-GCM, por NCC en 2020), pero no todas (`rsa` tuvo el ataque Marvin,
  RUSTSEC-2023-0071, por no ser de tiempo constante: aquí solo se usa para verificar firmas, que no toca secretos).
  Es más lento que `ring` o `aws-lc-rs`, que llevan ensamblador.
- Lo que no se sabe: la velocidad en la A72 (no se ha medido nada) y cuánta pila y montón gasta un saludo.

### B. rustls con `ring` o `aws-lc-rs`

- Gana: criptografía auditada y rápida (ensamblador con NEON).
- Cuesta: son C y ensamblador. `aws-lc-rs` necesita `std` y CMake: no. `ring` compila sin `std`, pero su ensamblador
  usa NEON y los programas de DashOS se compilan `softfloat`; habría que compilar ese crate con FPU y con el
  compilador cruzado de C (`toolchains/`), y comprobar que el kernel guarda los registros de la FPU de ese proceso
  (lo hace con los que la usan: `fpu_a` y `fpu_b` lo prueban). No se ha intentado. Licencia de `ring`: Apache-2.0 e
  ISC, con partes de OpenSSL (BSD con cláusula de publicidad en ficheros viejos): habría que mirarla fichero a fichero.

### C. embedded-tls 0.19 (una pila pequeña)

- Gana: pensada para sin `std` y sin montón; pequeña; Apache-2.0.
- Cuesta: solo AES-GCM (`TLS_AES_128_GCM_SHA256` y `TLS_AES_256_GCM_SHA384`): sin ChaCha20, justo lo que la Pi 4
  necesita. Su README dice que es trabajo en curso y que le faltan extensiones. La verificación de certificados es una
  opción aparte (`webpki`, o `rustpki` con RSA, P-384 y Ed25519 opcionales) y poco rodada. No se ha medido su tamaño.

### D. Escribir una propia

- No. TLS tiene demasiados casos raros y un fallo es grave.

## Recomendación

**A, con cautela declarada**: rustls sin `std` y el proveedor en Rust, sabiendo que la parte no auditada es la unión
de las primitivas, no el protocolo. Lo primero que se haga con él que no sea de riesgo (bajar una página, mirar el
tiempo). Antes de fiarle contraseñas o dinero, una de dos: pasar al proveedor `ring` (opción B, si se resuelve compilar
con FPU) o esperar a que `rustls-rustcrypto` salga de alfa. El cambio de proveedor no toca a los programas: es una
línea en quien crea la configuración.

## Dónde iría

- **Una biblioteca, no un servicio** (`usuario/src/tls.rs`, opción `tls` de `dashos-usuario`, como `red`): el TLS va
  dentro del programa que lo usa, encima de una `Conexion` de `red_cliente`. Así las claves de sesión y lo que se
  descifra no salen del proceso, y el servicio de red sigue viendo solo bytes cifrados. Cuesta 600 KB por programa que
  lo use; cuando haya bibliotecas compartidas (S11), una sola copia.
- **`amissl.library` nativa** para los 68k (S13, pregunta abierta 3): después, encima de esa biblioteca.

## Las raíces de confianza

Dos capas, como las asignaciones (S4b):

1. **Del sistema, en `SYS:`**: `SYS:Prefs/Confianza/` con un fichero por raíz (el certificado en DER) o uno solo con
   todas. Vienen con el sistema, firmadas con él, y se ponen al día con sus actualizaciones (S8). Solo las cambia un
   administrador ("añadir una clave de confianza" ya es una acción de administrador en S6b). Punto de partida: las de
   Mozilla (las que lleva `webpki-roots`; 58 KB compiladas).
2. **Del usuario, en `Home:Prefs/Confianza/`**: las que añade (la de su empresa, la de su servidor de casa) y una lista
   de las del sistema que **no** quiere. Valen solo para sus programas. Las lee la biblioteca por la tabla de nombres
   del proceso; un programa encerrado en su cajón no las ve y se queda con las del sistema.

Mientras no haya eso, las raíces compiladas dentro de la biblioteca (`webpki-roots`): caducan con ella, y hay que
decirlo.

Lo que falta decidir aparte: revocación (rustls admite CRL; OCSP grapado se puede exigir), y si se fijan claves
(pinning) para lo propio de DashOS (las actualizaciones).

## Estimación

Con una persona (o un agente) que ya conoce el servicio de red:

| Paso | Qué | Esfuerzo |
|---|---|---|
| 1 | Biblioteca `tls`: la configuración, el reloj, el azar, y un `ConexionTls` sobre `Conexion` (saludo, enviar, recibir, cerrar) | 2 a 3 días |
| 2 | Pruebas sin Internet: un servidor TLS en el PC (Python `ssl`, por `guestfwd`) con un certificado de una raíz de prueba; hostiles: certificado caducado, de otro nombre, de otra raíz, cadena rota, servidor que solo habla TLS 1.2, registros partidos y enormes | 2 a 3 días |
| 3 | Raíces en `SYS:` y la capa del usuario, con su prueba (una raíz quitada por el usuario deja de valer) | 2 días |
| 4 | Medir en la Pi 4: saludo (RSA 2048 y P-256) y caudal con ChaCha20 y con AES-GCM | 1 día, con placa |
| 5 | Si las medidas o el riesgo lo piden: el proveedor `ring` con FPU | 3 a 5 días, incierto |

Total sin el paso 5: **7 a 9 días**. Lo incierto es el paso 5 y lo que salga al hablar con servidores de verdad.

## Lo que no se comprobó

- Que el programa de la medida funcione: no se ejecutó.
- La velocidad, la pila y el montón.
- `embedded-tls` y `ring` no se compilaron para DashOS.
- Las auditorías citadas son de memoria de sus avisos públicos; antes de decidir, mirar las fechas y el alcance.
