#!/bin/bash
# DashOS: deshace tools/vfio_red_preparar.sh. Se ejecuta con sudo.
set -u
PCI=0000:03:00.0
OTRO=enp4s0
systemctl stop dashos-dhcp dashos-eco
echo $PCI > /sys/bus/pci/drivers/vfio-pci/unbind
echo > /sys/bus/pci/devices/$PCI/driver_override
echo $PCI > /sys/bus/pci/drivers_probe
if command -v ufw >/dev/null; then ufw delete allow in on $OTRO; fi
ip addr flush dev $OTRO
nmcli device set $OTRO managed yes
echo "Controlador de $PCI: $(basename "$(readlink /sys/bus/pci/devices/$PCI/driver)") (tiene que ser r8169)"
