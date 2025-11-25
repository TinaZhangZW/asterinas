#!/bin/sh

# SPDX-License-Identifier: MPL-2.0

INSTALL_DEVICE="${1:-/dev/vda}"

if [ $# -gt 1 ]; then
    echo "Usage: $0 [device_name]"
    echo "Example: $0 /dev/vda"
    exit 1
fi

if [ ! -b "$INSTALL_DEVICE" ]; then
    echo "Error: '$INSTALL_DEVICE' does not exist or is not a block device"
    exit 1
fi

sudo parted $INSTALL_DEVICE -- mklabel gpt
sudo parted $INSTALL_DEVICE -- mkpart ESP fat32 1MB 512MB
sudo parted $INSTALL_DEVICE -- mkpart root ext2 512MB 100%
sudo parted $INSTALL_DEVICE -- set 1 esp on

sudo mkfs.fat -F 32 -n boot "${INSTALL_DEVICE}1"
sudo mkfs.ext2 -L nixos "${INSTALL_DEVICE}2"

sudo mount -o sync,dirsync "${INSTALL_DEVICE}2" /mnt
sudo mkdir -p /mnt/boot
sudo mount -o umask=077,sync,dirsync "${INSTALL_DEVICE}1" /mnt/boot

sudo mkdir -p /mnt/etc/nixos
sudo cp /asterinas/configuration.nix /mnt/etc/nixos/configuration.nix
sudo cp -r /asterinas/overlays /mnt/etc/nixos/overlays

sudo nixos-install --no-root-passwd

sudo umount /mnt/boot
sudo umount /mnt

sudo shutdown -f now
