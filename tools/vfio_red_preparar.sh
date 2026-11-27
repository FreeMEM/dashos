#!/bin/bash
# DashOS: prepara shiva para probar rtl-red con la RTL8125 de verdad (pasos 1 y 2 de tools/vfio_red.sh, juntos).
# Se ejecuta con sudo. El DHCP y el eco quedan como servicios de systemd pasajeros (dashos-dhcp y dashos-eco); los
# quita tools/vfio_red_devolver.sh. Primer argumento: PID del proceso que lanzará la prueba (se le sube el límite de
# memoria bloqueable, que heredan sus hijos).
set -eu
PCI=0000:03:00.0
OTRO=enp4s0
QUIEN=${SUDO_USER:?ejecútalo con sudo}
PID_PRUEBA=${1:?falta el PID del proceso que lanzará la prueba}
ECO="$(cd "$(dirname "$0")" && pwd)/red_eco.py"
GRUPO=$(basename "$(readlink /sys/bus/pci/devices/$PCI/iommu_group)")

# El otro lado: enp4s0 fuera de NetworkManager, con las direcciones de la red de QEMU.
nmcli device set $OTRO managed no
ip addr flush dev $OTRO
ip addr add 10.0.2.2/24 dev $OTRO
ip addr add 10.0.2.100/24 dev $OTRO
ip link set $OTRO up
if command -v ufw >/dev/null; then ufw allow in on $OTRO; fi
systemd-run --unit=dashos-dhcp dnsmasq --no-daemon --conf-file=/dev/null --port=0 --interface=$OTRO \
  --bind-interfaces --except-interface=lo --dhcp-authoritative --dhcp-range=10.0.2.15,10.0.2.15,255.255.255.0,1h \
  --dhcp-option=3,10.0.2.2 --dhcp-option=6,10.0.2.3 --leasefile-ro --log-dhcp
systemd-run --unit=dashos-eco -p User="$QUIEN" -p AmbientCapabilities=CAP_NET_BIND_SERVICE \
  systemd-socket-activate --accept --inetd -l 10.0.2.100:7 python3 -I "$ECO"

# La tarjeta, de r8169 a vfio-pci.
modprobe vfio-pci
echo $PCI > /sys/bus/pci/devices/$PCI/driver/unbind
echo vfio-pci > /sys/bus/pci/devices/$PCI/driver_override
echo $PCI > /sys/bus/pci/drivers_probe
chown "$QUIEN" /dev/vfio/$GRUPO
prlimit --pid "$PID_PRUEBA" --memlock=unlimited:unlimited

echo "Controlador de $PCI: $(basename "$(readlink /sys/bus/pci/devices/$PCI/driver)") (tiene que ser vfio-pci)"
systemctl is-active dashos-dhcp dashos-eco
