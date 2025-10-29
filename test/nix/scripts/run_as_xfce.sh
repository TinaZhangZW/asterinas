#!/bin/sh
chmod a+rw /Desktop/*.desktop
ln -s /bin/thunar /bin/Thunar
export PATH="/run/current-system/sw/bin:/usr/sbin:/usr/bin:/sbin:/bin:${PATH:-}"

# Step 1: run dbus
#export DBUS_VERBOSE=1
#export DBUS_DEBUG_OUTPUT=1
export NO_AT_BRIDGE=1
chmod 755 /run/dbus
eval "$(/usr/bin/dbus-launch --sh-syntax)"

if command -v dconf-service >/dev/null 2>&1; then
  dconf-service > ~/dconf.log 2>&1 & echo $! > /run/dconf-service.pid &
fi

# Step 2: run Xorg
Xorg :0 -modulepath /usr/lib/xorg/modules -config /usr/share/X11/xorg.conf.d/10-fbdev.conf -logverbose 6 -logfile /var/xorg_debug.log -novtswitch -keeptty -keyboard keyboard -pointer mouse0 -xkbdir /usr/share/X11/xkb & echo $! > /run/xorg.pid &

# Step 3: run xfconfd
export DISPLAY=:0
export GDK_BACKEND=x11
export GDK_CORE_DEVICE_EVENTS=1

export GTK_THEME="Adwaita"
export ICON_THEME="hicolor"
export XDG_DATA_DIRS="/usr/share:/usr/local/share:/run/current-system/sw/share"
export GSETTINGS_SCHEMA_DIR="/usr/share/glib-2.0/schemas"

export GDK_PIXBUF_MODULE_FILE=/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache
export GDK_PIXBUF_MODULEDIR=/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders
export GIO_MODULE_DIR=/usr/lib/gio/modules
export GIO_EXTRA_MODULES=/usr/lib/gio/modules

#for debug
#export G_MESSAGES_DEBUG=all

xfce4-session &
