#!/bin/sh
# La caja Amiga en Linux (docs/CAJA_LINUX_LOOP.md): sus pruebas, con las dos cajas (make -C caja linux linux-x86): la
# de x86-64, nativa si el PC lo es, y la de AArch64, con qemu-aarch64 si no lo es. Escribe "BIEN" o "MAL" por cada una
# y acaba con el número de fallos.
cd "$(dirname "$0")" || exit 1
AQUI=$(pwd)
fallos=0

comprueba() {   # nombre, código esperado, salida, código, texto que tiene que salir
    if [ "$4" = "$2" ] && printf '%s\n' "$3" | grep -qF -- "$5"; then
        echo "  BIEN $1"
    else
        echo "  MAL  $1 (código $4, esperado $2)"; printf '%s\n' "$3" | sed 's/^/      /'
        fallos=$((fallos + 1))
    fi
}

prueba() {   # cómo lanzar la caja
    echo "$*"
    t=$(mktemp -d) || exit 1
    s=$(cd "$t" && CAJA_AMIGA_SYS="$t/sys" "$@" "$AQUI/../pruebas68k/saludo.exe" 2>&1); r=$?
    comprueba "saludo.exe (m68k-amigaos-gcc, por la dos.library)" 0 "$s" $r "Write() en Output(): bien"
    s=$(cd "$t" && mkdir -p sub && echo hola > hola.txt && CAJA_AMIGA_SYS="$t/sys" "$@" "$AQUI/cajones.exe" 2>&1); r=$?
    comprueba "cajones.exe: crear, escribir, renombrar y borrar" 0 "$s" $r "crear, escribir, renombrar y borrar: bien"
    comprueba "cajones.exe: el cajón actual es el de Linux" 0 "$s" $r "cajon actual: Linux:${t#/}"
    comprueba "cajones.exe: lista el cajón" 0 "$s" $r "fichero      5 hola.txt"
    s=$(cd "$t" && printf 'hola, caja\nañoranza\n' | CAJA_AMIGA_SYS="$t/sys" "$@" "$AQUI/consola.exe" 2>&1); r=$?
    comprueba "consola.exe: lee la entrada estándar (UTF-8 a Latin-1 y vuelta)" 0 "$s" $r "AÑORANZA"
    comprueba "consola.exe: cuenta los bytes en Latin-1" 0 "$s" $r "20 bytes leidos"
    s=$(cd "$t" && { CAJA_AMIGA_SYS="$t/sys" "$@" "$AQUI/consola.exe" espera & p=$!; sleep 2; kill -INT $p; wait $p; } 2>&1); r=$?
    comprueba "consola.exe espera: Ctrl-C llega como SIGBREAKF_CTRL_C" 5 "$s" $r "***Break"
    mkdir -p "$t/con espacio" && printf 'texto con ñ\n' > "$t/con espacio/año.txt"
    s=$(cd / && CAJA_AMIGA_SYS="$t/sys" "$@" "$AQUI/tipo.exe" "$t/con espacio/año.txt" 2>&1); r=$?
    comprueba "tipo.exe: una ruta absoluta de Linux pasa a Linux:, con comillas" 0 "$s" $r "args: [\"Linux:${t#/}/con espacio/año.txt\"]"
    comprueba "tipo.exe: el texto UTF-8 del fichero sale tal cual" 0 "$s" $r "texto con ñ"
    s=$(cd "$t" && CAJA_AMIGA_SYS="$t/sys" "$@" "$AQUI/tipo.exe" /no/existe 2>&1); r=$?
    comprueba "tipo.exe: lo que no existe en Linux queda como está" 10 "$s" $r "object not found"
    # Pruebas del oráculo: la salida de FS-UAE con el Kickstart (tools/emu_run.py de Dash), guardada en *.esperado.
    # Sin la guardia de ficheros (CAJA_AMIGA_PROTEGER=no): en un Amiga no la hay.
    for p in dosasl patron formato supervisor icono; do
        s=$(cd "$t" && CAJA_AMIGA_PROTEGER=no CAJA_AMIGA_SYS="$t/sys" "$@" "$AQUI/../pruebas68k/$p.exe" 2>&1); r=$?
        if [ $r = 0 ] && [ "$s" = "$(cat "$AQUI/$p.esperado")" ]; then echo "  BIEN $p.exe: como el oráculo"
        else echo "  MAL  $p.exe (código $r): no es como el oráculo"; printf '%s\n' "$s" | diff "$AQUI/$p.esperado" - | sed 's/^/      /'
            fallos=$((fallos + 1)); fi
    done
    # Con la guardia y sin terminal a quien preguntar: borrar un icono que ya estaba se niega.
    s=$(cd "$t" && CAJA_AMIGA_SYS="$t/sys" setsid "$@" "$AQUI/../pruebas68k/icono.exe" < /dev/null 2>&1); r=$?
    comprueba "icono.exe con la guardia y sin terminal: no borra el icono" 0 "$s" $r "DeleteDiskObject: 0"
    rm -f "$t/prueba_icono.info"
    s=$(cd "$t" && "$@" no-existe.exe 2>&1); r=$?
    comprueba "un programa que no está" 2 "$s" $r "no se puede leer"
    s=$(cd "$t" && "$@" "$AQUI/../pruebas68k/sistema.s" 2>&1); r=$?
    comprueba "algo que no es un ejecutable de AmigaOS" 2 "$s" $r "no es un ejecutable de AmigaOS"
    rm -rf "$t"
}

case "$(uname -m)" in
    x86_64) X86="" ; A64="qemu-aarch64" ;;
    aarch64) X86="qemu-x86_64" ; A64="" ;;
    *) X86="qemu-x86_64" ; A64="qemu-aarch64" ;;
esac
[ -x ../build-linux-x86/caja-amiga ] && prueba $X86 "$AQUI/../build-linux-x86/caja-amiga"
[ -x ../build-linux/caja-amiga ] && prueba $A64 "$AQUI/../build-linux/caja-amiga"
echo "fallos: $fallos"
exit $fallos
