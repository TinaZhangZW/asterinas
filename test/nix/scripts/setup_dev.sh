mknod /dev/null c 1 3
mknod /dev/zero c 1 5
mknod /dev/tty c 5 0
mknod /dev/random c 1 8
mknod /dev/urandom c 1 9
mknod /dev/fb0 c 29 0

# Set permissions for the device nodes
chmod 666 /dev/null
chmod 666 /dev/zero
chmod 666 /dev/tty
chmod 666 /dev/random
chmod 666 /dev/urandom
chmod 666 /dev/fb0

# The goal is to make them align with Asterinas input subsystem where
# we have hardcoded major and minor numbers for input devices.
mkdir -p /dev/input
mknod /dev/input/event0 c 13 65 # AT Translated Set 2 keyboard
mknod /dev/input/event1 c 13 66 # PS/2 Generic Mouse
mknod /dev/input/event2 c 13 64 # Power Button
chmod 644 /dev/input/event*

# These are needed for xfce terminal app
mkdir -p /dev/pts
mount -t devpts devpts /dev/pts
mknod /dev/ptmx c 5 2