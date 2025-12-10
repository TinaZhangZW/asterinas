#!/bin/sh

cat <<'EOF'
systemd 257.5 running in system mode (-PAM -AUDIT -SELINUX -APPARMOR +IMA +IPE +SMACK +SECCOMP -GCRYPT -GNUTLS -OPENSSL -ACL +BLKID -CURL -ELFUTILS -FIDO2 -IDN2 -IDN -IPTC +KMOD -LIBCRYPTSETUP -LIBCRYPTSETUP_PLUGINS +LIBFDISK -PCRE2 -PWQUALITY -P11KIT -QRENCODE -TPM2 -BZIP2 -LZ4 -XZ -ZLIB -ZSTD -BPF_FRAMEWORK -BTF -XKBCOMMON +UTMP -SYSVINIT -LIBARCHIVE)
Detected virtualization kvm.
Detected architecture x86-64.
warning!!!!! timerfd_settime: TFD_TIMER_CANCEL_ON_SET is not supported
Queued start job for default target Graphical Interface.
[  OK  ] Started Dispatch Password Requests to Console Directory Watch.
[  OK  ] Started Forward Password Requests to Wall Directory Watch.
[  OK  ] Reached target Login Prompts.
[  OK  ] Reached target Local File Systems.
[  OK  ] Reached target Path Units.
[  OK  ] Reached target Slice Units.
[  OK  ] Reached target Swaps.
[  OK  ] Started XFCE Desktop Environment.
[  OK  ] Reached target Graphical Interface.
System is tainted: var-run-bad
[  OK  ] Reached target Multi-User System.
[  OK  ] Started XFCE Desktop Environment.
[  OK  ] Reached target Graphical Interface.
Startup finished in 10.515s (kernel) + 20ms (userspace) = 10.535s.
EOF

#!/bin/sh
#chmod a+rw /Desktop/*.desktop
#ln -s /bin/thunar /bin/Thunar
export PATH="/run/current-system/sw/bin:/usr/sbin:/usr/bin:/sbin:/bin:${PATH:-}"
HOME=/root

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
Xorg :0 -modulepath /usr/lib/xorg/modules -config /usr/share/X11/xorg.conf.d/10-fbdev.conf -logverbose 0 -logfile /var/xorg_debug.log -novtswitch -keeptty -keyboard keyboard -pointer mouse0 -xkbdir /usr/share/X11/xkb & echo $! > /run/xorg.pid &

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

