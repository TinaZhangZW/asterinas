#!/bin/bash

rm -f disk.img
dd if=/dev/zero of=disk.img bs=1M count=4096

qemu-system-x86_64 \
	-cpu host -smp 1 -m 8G -enable-kvm \
	-bios /root/ovmf/release/OVMF.fd \
	-cdrom nixos-minimal-25.05pre-git-x86_64-linux.iso -boot d \
	-drive if=virtio,format=raw,file=disk.img \
	-nographic -display vnc=0.0.0.0:21 \
	-chardev stdio,id=mux,mux=on,signal=off,logfile=qemu.log \
	-serial chardev:mux -monitor chardev:mux \
	-device virtio-serial-pci,disable-legacy=on,disable-modern=off \
	-device virtconsole,chardev=mux

