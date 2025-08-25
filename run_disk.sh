#!/bin/bash

qemu-system-x86_64 \
	-cpu host -smp 1 -m 8G -enable-kvm\
	-bios /root/ovmf/release/OVMF.fd \
	-drive if=none,format=raw,id=x0,file=disk.img \
	-device virtio-blk-pci,drive=x0,disable-legacy=on,disable-modern=off \
	-nographic -display vnc=0.0.0.0:21 \
	-serial chardev:mux -monitor chardev:mux \
	-chardev stdio,id=mux,mux=on,signal=off,logfile=qemu.log \
	-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
	-device virtio-serial-pci,disable-legacy=on,disable-modern=off \
	-device virtconsole,chardev=mux

