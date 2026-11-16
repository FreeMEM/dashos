#!/usr/bin/env bash
# DashOS (X5c): prepara la prueba de una tarjeta de red de verdad pasada a QEMU con VFIO (la RTL8125 de shiva, con el
# controlador `rtl-red`). NO CAMBIA NADA DEL SISTEMA NI USA SUDO: comprueba lo que se puede comprobar sin privilegios
# e IMPRIME los comandos que tiene que ejecutar el usuario, en orden.
#
#   tools/vfio_red.sh [PCI] [OTRO]
#
# PCI: la tarjeta que se pasa (por omisión 0000:03:00.0, la RTL8125 de shiva). OTRO: la interfaz de Linux al otro lado
# del cable (por omisión enp4s0, la I226-V de shiva), que hace de red de usuario de QEMU: 10.0.2.2 (puerta), DHCP que
# da 10.0.2.15 y un eco en 10.0.2.100:7 (`tools/red_eco.py`), como espera `red-prueba`.
#
# Se niega (sale con 1) si la tarjeta no está sola en su grupo de la IOMMU o si es la interfaz de la ruta por defecto;
# y si OTRO es la de la ruta por defecto (se le quitan sus direcciones).
set -u
PCI=${1:-0000:03:00.0}
OTRO=${2:-enp4s0}
RAIZ=$(cd "$(dirname "$0")/.." && pwd)
DEV=/sys/bus/pci/devices/$PCI
MAL=0

mal() { echo "NO: $*"; MAL=1; }
aviso() { echo "OJO: $*"; }

echo "== Comprobaciones (sin privilegios) =="
if [ ! -d "$DEV" ]; then
    echo "NO: no hay aparato PCI $PCI"
    exit 1
fi
ID="$(cat "$DEV/vendor"):$(cat "$DEV/device")"
echo "Aparato $PCI: $ID, revisión $(cat "$DEV/revision")"
[ "$ID" = "0x10ec:0x8125" ] || aviso "no es una RTL8125 (10ec:8125); rtl-red no lo atenderá"

# El grupo de la IOMMU: la tarjeta tiene que estar sola (si no, VFIO pide pasar el grupo entero).
if [ ! -e "$DEV/iommu_group" ]; then
    mal "$PCI no tiene grupo de IOMMU (¿IOMMU apagada en la BIOS o en la línea del kernel?)"
    GRUPO=""
else
    GRUPO=$(basename "$(readlink -f "$DEV/iommu_group")")
    EN_GRUPO=()
    for d in /sys/kernel/iommu_groups/"$GRUPO"/devices/*; do EN_GRUPO+=("$(basename "$d")"); done
    if [ "${#EN_GRUPO[@]}" -eq 1 ] && [ "${EN_GRUPO[0]}" = "$PCI" ]; then
        echo "Grupo de IOMMU $GRUPO: solo $PCI (bien)"
    else
        mal "el grupo de IOMMU $GRUPO tiene más aparatos: ${EN_GRUPO[*]}"
    fi
fi

# Quién la lleva ahora y qué interfaz de Linux es.
DRIVER=""
[ -e "$DEV/driver" ] && DRIVER=$(basename "$(readlink -f "$DEV/driver")")
echo "Controlador de Linux ahora: ${DRIVER:-ninguno}"
IFACE=""
for n in "$DEV"/net/*; do [ -e "$n" ] && IFACE=$(basename "$n"); done
MAC=""
if [ -n "$IFACE" ]; then
    MAC=$(cat "$DEV/net/$IFACE/address")
    echo "Interfaz: $IFACE, MAC $MAC, enlace $(cat "$DEV/net/$IFACE/operstate")"
fi

# La ruta por defecto no puede ir por ninguna de las dos.
POR_DEFECTO=$(ip -o route show default 2>/dev/null | awk '{for (i = 1; i < NF; i++) if ($i == "dev") print $(i + 1)}' | sort -u)
echo "Ruta por defecto por: ${POR_DEFECTO:-ninguna}"
for i in $POR_DEFECTO; do
    [ -n "$IFACE" ] && [ "$i" = "$IFACE" ] && mal "$IFACE ($PCI) lleva la ruta por defecto: pasarla cortaría la red"
    [ "$i" = "$OTRO" ] && mal "$OTRO lleva la ruta por defecto: no se puede usar como el otro lado"
done
if [ -n "$IFACE" ] && [ -n "$(ip -o addr show dev "$IFACE" 2>/dev/null)" ]; then
    aviso "$IFACE tiene direcciones: $(ip -o addr show dev "$IFACE" | awk '{print $4}' | tr '\n' ' ')(se pierden al soltarla)"
fi
if [ ! -d "/sys/class/net/$OTRO" ]; then
    mal "no hay interfaz $OTRO para el otro lado"
else
    echo "Otro lado: $OTRO ($(basename "$(readlink -f "/sys/class/net/$OTRO/device/driver" 2>/dev/null)" 2>/dev/null)), enlace $(cat "/sys/class/net/$OTRO/operstate")"
    if [ -n "$(ip -o addr show dev "$OTRO" 2>/dev/null)" ]; then
        aviso "$OTRO tiene direcciones: $(ip -o addr show dev "$OTRO" | awk '{print $4}' | tr '\n' ' ')(se le quitan)"
    fi
fi

# Lo que hace falta para VFIO y QEMU con KVM.
[ -e /dev/kvm ] || mal "no hay /dev/kvm"
[ -e /dev/vfio/vfio ] || aviso "no hay /dev/vfio/vfio (sale con modprobe vfio-pci)"
[ -d /sys/module/vfio_pci ] || echo "El módulo vfio_pci no está cargado (se carga abajo)"
MEMLOCK=$(ulimit -l)
echo "Memoria bloqueable (ulimit -l): $MEMLOCK KB (QEMU bloquea los 2 GB del invitado con VFIO)"
for p in dnsmasq systemd-socket-activate; do command -v $p >/dev/null || mal "falta $p"; done
UFW=$(systemctl is-active ufw 2>/dev/null)
FIREWALLD=$(systemctl is-active firewalld 2>/dev/null)
NM=$(systemctl is-active NetworkManager 2>/dev/null)

if [ "$MAL" -ne 0 ]; then
    echo
    echo "No se sigue: arregla lo de arriba."
    exit 1
fi

DEVVFIO=/dev/vfio/$GRUPO
cat <<EOF

== 1. El otro lado ($OTRO, en Linux), cada orden en una terminal aparte si se queda en marcha ==
EOF
[ "$NM" = active ] && echo "sudo nmcli device set $OTRO managed no"
cat <<EOF
sudo ip addr flush dev $OTRO
sudo ip addr add 10.0.2.2/24 dev $OTRO
sudo ip addr add 10.0.2.100/24 dev $OTRO
sudo ip link set $OTRO up
EOF
[ "$UFW" = active ] && echo "sudo ufw allow in on $OTRO"
[ "$FIREWALLD" = active ] && echo "sudo firewall-cmd --zone=trusted --add-interface=$OTRO"
cat <<EOF
# DHCP (se queda en marcha; Ctrl-C para pararlo): siempre 10.0.2.15, puerta 10.0.2.2, DNS 10.0.2.3, sin DNS propio.
sudo dnsmasq --no-daemon --conf-file=/dev/null --port=0 --interface=$OTRO --bind-interfaces --except-interface=lo --dhcp-authoritative --dhcp-range=10.0.2.15,10.0.2.15,255.255.255.0,1h --dhcp-option=3,10.0.2.2 --dhcp-option=6,10.0.2.3 --leasefile-ro --log-dhcp
# El eco en 10.0.2.100:7 (se queda en marcha; Ctrl-C para pararlo): un tools/red_eco.py por conexión.
sudo systemd-socket-activate --accept --inetd -l 10.0.2.100:7 python3 -I $RAIZ/tools/red_eco.py

== 2. Soltar $PCI de ${DRIVER:-su controlador} y dársela a vfio-pci ==
sudo modprobe vfio-pci
EOF
[ -n "$DRIVER" ] && echo "echo $PCI | sudo tee $DEV/driver/unbind"
cat <<EOF
echo vfio-pci | sudo tee $DEV/driver_override
echo $PCI | sudo tee /sys/bus/pci/drivers_probe
sudo chown $(id -un) $DEVVFIO
# QEMU bloquea la memoria del invitado: en la terminal que lanzará make, sube su límite (\$\$ es esa terminal).
sudo prlimit --pid \$\$ --memlock=unlimited:unlimited
# Comprobar: tiene que decir vfio-pci.
basename \$(readlink $DEV/driver)

== 3. La prueba (en la terminal del prlimit, desde $RAIZ) ==
make -C kernel arranque-x86 RED_X86=vfio PCI_RED=$PCI ACCEL=kvm${MAC:+ MAC_RED=$MAC}
# Con la VT-d de QEMU (la IOMMU del invitado), además IOMMU=intel. La salida queda en kernel/target/salida_x86.txt;
# las líneas de [rtl-red] dicen la versión, la MAC, el PHY, el firmware y el enlace (docs/spec/hw/rtl8125.md, 16).

== 4. Devolverlo todo ==
# Parar el eco y dnsmasq (Ctrl-C en sus terminales).
echo $PCI | sudo tee /sys/bus/pci/drivers/vfio-pci/unbind
echo | sudo tee $DEV/driver_override
echo $PCI | sudo tee /sys/bus/pci/drivers_probe
EOF
[ "$UFW" = active ] && echo "sudo ufw delete allow in on $OTRO"
[ "$FIREWALLD" = active ] && echo "sudo firewall-cmd --zone=trusted --remove-interface=$OTRO"
echo "sudo ip addr flush dev $OTRO"
[ "$NM" = active ] && echo "sudo nmcli device set $OTRO managed yes"
echo "# Comprobar: tiene que decir ${DRIVER:-r8169}."
echo "basename \$(readlink $DEV/driver)"
