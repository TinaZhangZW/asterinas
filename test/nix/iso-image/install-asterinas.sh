#!/bin/sh

# SPDX-License-Identifier: MPL-2.0

if [ $# -eq 0 ]; then
    echo "Usage: $0 <device_name>"
    echo "Example: $0 /dev/sda"
    exit 1
fi

INSATLL_DEVICE="$1"
if [ ! -b "$INSATLL_DEVICE" ]; then
    echo "Error: '$INSATLL_DEVICE' does not exist or is not a block device"
    exit 1
fi

parted $INSATLL_DEVICE -- mklabel gpt
parted $INSATLL_DEVICE -- mkpart ESP fat32 1MB 512MB
parted $INSATLL_DEVICE -- mkpart root ext2 512MB 100%
parted $INSATLL_DEVICE -- set 1 esp on

mkfs.fat -F 32 -n boot "${INSATLL_DEVICE}1"
mkfs.ext2 -L nixos "${INSATLL_DEVICE}2"

mount -o sync,dirsync "${INSATLL_DEVICE}2" /mnt
mkdir -p /mnt/boot
mount -o umask=077,sync,dirsync "${INSATLL_DEVICE}1" /mnt/boot

mkdir -p /mnt/etc/nixos
cp /asterinas/configuration.nix /mnt/etc/nixos/configuration.nix
if [ -f /asterinas/xorg.nix ]; then
    cp /asterinas/xorg.nix /mnt/etc/nixos/xorg.nix
fi
if [ -f /asterinas/xfce.nix ]; then
    cp /asterinas/xfce.nix /mnt/etc/nixos/xfce.nix
fi
if [ -f /asterinas/systemd.nix ]; then
    cp /asterinas/systemd.nix /mnt/etc/nixos/systemd.nix
fi
if [ -d /asterinas/patches ]; then
    cp -r /asterinas/patches /mnt/etc/nixos/patches
fi
if [ -f /asterinas/run_as_xfce.sh ]; then
    cp /asterinas/run_as_xfce.sh /mnt/etc/nixos/run_as_xfce.sh
fi

nixos-install --no-root-passwd

umount /mnt/boot
umount /mnt

exit 0
