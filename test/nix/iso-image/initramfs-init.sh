#!/bin/sh

# SPDX-License-Identifier: MPL-2.0

NEW_ROOT=""
NEW_INIT=""
BREAK=""
ARGS=""

for arg in "$@"; do
    case "$arg" in
        root=*)
            NEW_ROOT="${arg#root=}"
            ;;
        init=*)
            NEW_INIT="${arg#init=}"
            ;;
        rd.break=*)
            BREAK="${arg#rd.break=}"
            ;;
        *)
            ARGS="$ARGS $arg"
            ;;
    esac
done

echo "start to setup env ..."
mkdir -p /dev
mount -t devtmpfs devtmpfs /dev

mknod /dev/null c 1 3
mknod /dev/zero c 1 5
mknod /dev/tty c 5 0
mknod /dev/random c 1 8
mknod /dev/urandom c 1 9
mknod /dev/fb0 c 29 0

chmod 666 /dev/null
chmod 666 /dev/zero
chmod 666 /dev/tty
chmod 666 /dev/random
chmod 666 /dev/urandom
chmod 666 /dev/fb0

mknod /dev/tty0 c 4 0
mknod /dev/tty1 c 4 1

mknod /dev/vda b 253 0
mknod /dev/vda1 b 253 1
mknod /dev/vda2 b 253 2

mknod /dev/hvc0 c 229 0
mknod /dev/ttyS0 c 4 64


echo "NEW_ROOT: $NEW_ROOT"
echo "NEW_INIT: $NEW_INIT"
echo "BREAK: $BREAK"
echo "ARGS: $ARGS"

if [ "$BREAK" = "1" ]; then
    echo "Breaking into initramfs shell..."
    exec /bin/sh
fi

if [ -z "$NEW_ROOT" ] || [ -z "$NEW_INIT" ]; then
    echo "Error: 'root=' and 'init=' parameters are required."
    exit 1
fi

mkdir /sysroot
mount -t ext2 $NEW_ROOT /sysroot
mount -t sysfs none /sysroot/sys
mount -t proc none /sysroot/proc
mount --move /dev /sysroot/dev

mkdir -p /sysroot/run
mount -t tmpfs -o mode=0755 tmpfs /sysroot/run
ln -sf /nix/var/nix/profiles/system /sysroot/run/current-system

chroot /sysroot /run/current-system/sw/bin/localedef -i en_US -f UTF-8 en_US.UTF-8 || true

# Drain any pending terminal input

if [ -t 0 ]; then
    while read -t 0 -n 1 _ >/dev/null 2>&1; do :; done
fi
for dev in /dev/tty /dev/console /dev/ttyS0 /dev/hvc0; do
    [ -e "$dev" ] || continue
    while read -t 0 -n 1 _ < "$dev" >/dev/null 2>&1; do :; done
done


exec switch_root /sysroot $NEW_INIT $ARGS
