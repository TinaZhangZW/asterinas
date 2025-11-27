#!/bin/sh
# Step 1: run dbus
mkdir -p /var/lib/dbus /usr/share/X11/xorg.conf.d
[ -f /var/lib/dbus/machine-id ] || echo "52e0ad0e9794402c90315dd6af205511" > /var/lib/dbus/machine-id

if command -v dbus-launch >/dev/null 2>&1; then
  eval "$(dbus-launch --sh-syntax)"
fi

# Step 2:run Xorg
XKB_DATA="/run/current-system/sw/share/X11/xkb"
MODULE_PATH="/run/current-system/sw/lib/xorg/modules"

Xorg :0 \
  -modulepath "$MODULE_PATH" \
  -xkbdir "$XKB_DATA" \
  -logverbose 6 \
  -logfile /var/xorg_debug.log \
  -novtswitch \
  -keeptty \
  -keyboard keyboard \
  -pointer mouse0 &


# Step 3: run xfce4
export DISPLAY=:0
xfce4-session &
