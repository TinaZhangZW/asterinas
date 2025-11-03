#!/bin/sh
#chmod a+rw /Desktop/*.desktop
#ln -s /bin/thunar /bin/Thunar
source ./setup_env.sh
ln -s /nix/var/nix/profiles/system /run/current-system
export PATH="/run/current-system/sw/bin:/nix/var/nix/profiles/system/sw/bin:/usr/sbin:/usr/bin:/sbin:/bin:${PATH:-}"

# --- FIX START ---
# 1. Set HOME and XAUTHORITY so auth files can be created.
: "${HOME:=/root}" # Default to /root if HOME is not set
export HOME
export XAUTHORITY="${HOME}/.Xauthority"
mkdir -p "$HOME"
touch "$XAUTHORITY"
chmod 600 "$XAUTHORITY"

# Step 1: run dbus
#export DBUS_VERBOSE=1
#export DBUS_DEBUG_OUTPUT=1
export NO_AT_BRIDGE=1
#chmod 755 /run/dbus
mkdir -p /var/lib/dbus /usr/share/X11/xorg.conf.d
[ -f /var/lib/dbus/machine-id ] || echo "52e0ad0e9794402c90315dd6af205511" > /var/lib/dbus/machine-id
#mkdir -p /run/current-system/sw/bin
#cp /nix/var/nix/profiles/system/sw/bin/dbus-daemon /run/current-system/sw/bin/dbus-daemon
if command -v dbus-launch >/dev/null 2>&1; then
  eval "$(dbus-launch --sh-syntax)"
fi

#cp /nix/store/0rq9g4rvssd2ldp9x854h49fn0gb2l43-dconf-0.40.0-lib/libexec/dconf-service /usr/bin/dconf-service
#if command -v dconf-service >/dev/null 2>&1; then
  /nix/store/0rq9g4rvssd2ldp9x854h49fn0gb2l43-dconf-0.40.0-lib/libexec/dconf-service >/var/log/dconf-service.log 2>&1 &
#fi
#if command -v xfconfd >/dev/null 2>&1; then
  /nix/store/7xzw1ykyc4fpy9p9pfdfzxmm4zhiv3da-xfconf-4.20.0/lib/xfce4/xfconf/xfconfd >/var/log/xfconfd.log 2>&1 &
#fi

#export XDG_CONFIG_DIRS="/etc/xdg:/run/current-system/sw/etc:/run/current-system/sw/etc/xdg:${XDG_CONFIG_DIRS:-}"
export XDG_DATA_DIRS="/run/current-system/sw/share:/usr/share:/usr/local/share:${XDG_DATA_DIRS:-}"
export GSETTINGS_SCHEMA_DIR="/run/current-system/sw/share/glib-2.0/schemas:/usr/share/glib-2.0/schemas:${GSETTINGS_SCHEMA_DIR:-}"

# Step 2: run Xorg
#cp /nix/store/9d13lbjjw4hb25wsp07s84d530zf5gz0-system-path/bin/xkbcomp /usr/bin/xkbcomp
MODULEPATH="$(dirname "$(command -v Xorg)")/../lib/xorg/modules"
XKBDIR="/run/current-system/sw/share/X11/xkb"
mkdir -p /run/current-system/sw/share/X11
ln -s /nix/var/nix/profiles/system/sw/bin/xkbcomp /usr/bin/xkbcomp
ln -s /nix/store/6fq80h0myz0nc01425zm9hrfcl2p5hd7-xkeyboard-config-2.44/share/X11/xkb /run/current-system/sw/share/X11/xkb
Xorg :0 -modulepath "$MODULEPATH" -config /nix/store/v53w1aicazlbca6qgf0p1nbq06r70jrh-etc/etc/X11/xorg.conf.d/10-fbdev.conf -logverbose 6 -logfile /var/xorg_debug.log -novtswitch -keeptty -keyboard keyboard -pointer mouse0 -xkbdir "$XKBDIR" &
#Xorg :0 -modulepath /nix/store/9d13lbjjw4hb25wsp07s84d530zf5gz0-system-path/lib/xorg/modules -config /usr/share/X11/xorg.conf.d/10-fbdev.conf -logverbose 6 -logfile /var/xorg_debug.log -novtswitch -keeptty -keyboard keyboard -pointer mouse0 -xkbdir /nix/store/6fq80h0myz0nc01425zm9hrfcl2p5hd7-xkeyboard-config-2.44/share/X11/xkb &

# Step 3: run xfconfd
export DISPLAY=:0
export GDK_BACKEND=x11
export GDK_CORE_DEVICE_EVENTS=1
export GTK_THEME="Adwaita"
export ICON_THEME="hicolor"
#export XDG_DATA_DIRS="/usr/share:/usr/local/share:/run/current-system/sw/share"
#export GSETTINGS_SCHEMA_DIR="/usr/share/glib-2.0/schemas"

export GDK_PIXBUF_MODULE_FILE=/home/tina-dell/workspace/ant/asterinas/temp/nix/store/14spdmgq38vmzywkkm65s65ab6923y6p-librsvg-2.60.0/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache
#export GDK_PIXBUF_MODULEDIR=/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders
#export GIO_MODULE_DIR=/usr/lib/gio/modules
#export GIO_EXTRA_MODULES=/usr/lib/gio/modules

#for debug
#export G_MESSAGES_DEBUG=all

xfce4-session &
