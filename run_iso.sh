#!/bin/bash

rm -f asterinas.img
dd if=/dev/zero of=asterinas.img bs=1M count=8192

qemu-system-x86_64 \
	-cpu host -smp 1 -m 8G -enable-kvm \
	-bios /root/ovmf/release/OVMF.fd \
	-cdrom asterinas-installer.iso -boot d \
	-drive if=virtio,format=raw,file=asterinas.img \
	-nographic -display vnc=0.0.0.0:21 \
	-chardev stdio,id=mux,mux=on,signal=off,logfile=qemu.log \
	-serial chardev:mux -monitor chardev:mux \
	-device virtio-serial-pci,disable-legacy=on,disable-modern=off \
	-device virtconsole,chardev=mux
