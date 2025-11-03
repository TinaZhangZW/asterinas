{ config, lib, pkgs, ... }:
let
  xfce = import ./xfce.nix { inherit pkgs; };
  xorg = import ./xorg.nix { inherit pkgs; };
in {
  options = {
    asterinas.splash = lib.mkOption {
      type = lib.types.path;
      default = /asterinas/splash.png;
    };
    asterinas.kernel = lib.mkOption {
      type = lib.types.path;
      default = /asterinas/kernel;
    };
    asterinas.kernel-params = lib.mkOption {
      type = lib.types.str;
      default =
        "PATH=/bin:/nix/var/nix/profiles/system/sw/bin ostd.log_level=error -- sh /init root=/dev/vda2 init=/bin/sh rd.break=0";
    };
    asterinas.initramfsCompressed = lib.mkOption {
      type = lib.types.bool;
      default = false;
    };
    asterinas.initramfs-init = lib.mkOption {
      type = lib.types.path;
      default = /asterinas/initramfs-init.sh;
    };
    asterinas.initramfs = lib.mkOption {
      type = lib.types.path;
      default = pkgs.makeInitrd {
        compressor =
          if config.asterinas.initramfsCompressed then "gzip" else "cat";
        contents = [
          {
            object = "${pkgs.busybox}/bin";
            symlink = "/bin";
          }
          {
            object = "${config.asterinas.initramfs-init}";
            symlink = "/init";
          }
        ];
      };
    };
  };

  config = {
    systemd.package = pkgs.systemdMinimal;

    boot.loader.grub.enable = true;
    boot.loader.grub.efiSupport = true;
    boot.loader.grub.device = "nodev";
    boot.loader.grub.efiInstallAsRemovable = true;
    boot.loader.grub.splashImage = config.asterinas.splash;

    boot.initrd.enable = false;
    boot.kernel.enable = false;
    boot.loader.grub.extraInstallCommands = ''
      sed -i -E "s/(init=)[^[:space:]]+/\1\/bin\/busybox/" /boot/grub/grub.cfg
    '';
    system.systemBuilderCommands = ''
      echo ${config.asterinas.kernel-params} > $out/kernel-params
      ln -s ${config.asterinas.kernel} $out/kernel
      ln -s ${config.asterinas.initramfs}/initrd $out/initrd
    '';

    # Workaround for "Failed to set up credentials" for getty.
    # This manually creates the minimal user database files that systemd needs.
    environment.etc."passwd".text = ''
      root:x:0:0:root:/root:/bin/sh
    '';
    environment.etc."group".text = ''
      root:x:0:
    '';
    # This shadow entry for root has an empty password field, allowing login without a password.
    environment.etc."shadow".text = ''
      root::19234:0:99999:7:::
    '';

    systemd.services."xfce-desktop" = {
      description = "XFCE Desktop Environment";
      after = [ "multi-user.target" ];
      wantedBy = [ "graphical.target" ];
      serviceConfig = {
        Environment = "DISPLAY=:0";
        ExecStart = "/run/current-system/sw/bin/run_as_xfce";
        StandardOutput = "tty";
        StandardError = "tty";
        KillMode = "process";
        Delegate = "yes";
        Restart = "no";
        Type = "simple";
      };
    };

    systemd.enableCgroupAccounting = false;
    # Disable services not included in systemdMinimal to prevent build errors.
    # The 'boot.coredump.enable' line is removed because the option does not exist when using systemdMinimal.
    services.timesyncd.enable = false;

    systemd.oomd.enable = false;
    systemd.services.dhcpcd.enable = false;
    systemd.services."systemd-networkd".enable = false;
    systemd.services."NetworkManager".enable = false;
    systemd.services."wpa_supplicant".enable = false;
    systemd.services."systemd-journald".enable = false;
    systemd.services."systemd-udevd".enable = false;
    systemd.services."systemd-tmpfiles-setup".enable = false;

    system.activationScripts.maskSystemdUnits.text = ''
      #!/bin/sh
      set -e
      # This script masks systemd units by symlinking them to /dev/null.
      for u in \
        sys-fs-fuse-connections.mount \
        systemd-creds.socket \
        systemd-creds@.service \
        sockets.target.wants/systemd-creds.socket \
        systemd-remount-fs.service \
        systemd-firstboot.service \
        systemd-random-seed.service \
        systemd-update-utmp.service \
        sys-kernel-config.mount \
        sys-kernel-debug.mount \
        sys-kernel-tracing.mount \
        tmp.mount \
        local-fs.target.wants/tmp.mount \
        systemd-update-done.service \
        nscd.service \
        resolvconf.service \
        network-setup.service \
        network-setup.target \
        network-setup.path \
        network-setup.timer \
        network-online.target \
        network-pre.target \
        network-local-commands.service \
        network.target \
        network-online.target.wants/dhcpcd.service \
        network.target.wants/network-local-commands.service \
        systemd-journal-catalog-update.service \
        systemd-journal-flush.service \
        systemd-journald-audit.socket \
        systemd-journald-dev-log.socket \
        systemd-journald-sync@.service \
        systemd-journald-varlink@.socket \
        systemd-journald.socket \
        systemd-journald@.service \
        systemd-journald@.socket \
        systemd-journal-flush.service.d/overrides.conf \
        systemd-journald-audit.socket.d/overrides.conf \
        systemd-journald.service.wants/systemd-journald-audit.socket \
        systemd-journald@.service.d/overrides.conf \
        systemd-udevd.service \
        systemd-udev-trigger.service \
        systemd-udev-settle.service \
        systemd-udevd-control.socket \
        systemd-udevd-kernel.socket \
        systemd-udev-settle.service.d/overrides.conf \
        systemd-networkd.service \
        systemd-modprobe@.service \
        modprobe@.service \
        sysinit.target.wants/sys-fs-fuse-connections.mount \
        sysinit.target.wants/sys-kernel-config.mount \
        sysinit.target.wants/sys-kernel-debug.mount \
        sysinit.target.wants/sys-kernel-tracing.mount \
        sysinit.target.wants/systemd-update-done.service \
        systemd-tmpfiles-setup.service \
        systemd-tmpfiles-clean.service \
        systemd-tmpfiles-clean.timer \
        systemd-tmpfiles-resetup.service \
        systemd-tmpfiles-setup-dev-early.service \
        systemd-tmpfiles-setup-dev.service; do
        mkdir -p "/etc/systemd/system/$(dirname "$u")"
        ln -sf /dev/null "/etc/systemd/system/$u"
      done
      if command -v systemctl >/dev/null 2>&1; then
        systemctl daemon-reload || true
      fi
    '';

    environment.variables = {
      XDG_DATA_DIRS = "/run/current-system/sw/share:/usr/share:/usr/local/share";
      XDG_CONFIG_DIRS = "/etc/xdg:/run/current-system/sw/etc/xdg:/usr/local/etc/xdg";
      GSETTINGS_SCHEMA_DIR = "/run/current-system/sw/share/glib-2.0/schemas:/usr/share/glib-2.0/schemas";
    };
    environment.sessionVariables = lib.mkForce {
      XDG_DATA_DIRS = "/run/current-system/sw/share:/usr/share:/usr/local/share";
      XDG_CONFIG_DIRS = "/etc/xdg:/run/current-system/sw/etc/xdg:/usr/local/etc/xdg";
      GSETTINGS_SCHEMA_DIR = "/run/current-system/sw/share/glib-2.0/schemas:/usr/share/glib-2.0/schemas";
    };
    environment.etc."X11/xorg.conf.d/10-fbdev.conf".source = ./patches/xorgServer/10-fbdev.conf;
    environment.systemPackages = [
      # basic
      pkgs.dbus
      pkgs.hicolor-icon-theme
      pkgs.evtest
      pkgs.adwaita-icon-theme
      pkgs.gdk-pixbuf
      pkgs.gdk-pixbuf.dev
      pkgs.gdk-pixbuf-xlib
      pkgs.librsvg
      pkgs.libjpeg
      pkgs.libpng
      pkgs.shared-mime-info
      pkgs.dconf
      pkgs.gsettings-desktop-schemas
      pkgs.glib
      pkgs.glib.bin
      pkgs.glib-networking
      # xfce
      xfce.xfdesktop
      xfce.xfwm4
      pkgs.xfce.xfconf
      pkgs.xfce.xfce4-panel
      pkgs.xfce.thunar
      pkgs.xfce.mousepad
      pkgs.xfce.xfce4-appfinder
      pkgs.xfce.xfce4-settings
      pkgs.xfce.exo
      pkgs.xfce.tumbler
      pkgs.gvfs
      pkgs.xfce.xfce4-session
      pkgs.dconf.lib
      # Xorg server and basic drivers
      xorg.xtrans
      xorg.xcbproto
      xorg.xorgproto
      xorg.libxcb
      xorg.libx11
      xorg.libevdev
      xorg.evtest
      xorg.xorgServer
      pkgs.xorg.xf86videofbdev
      pkgs.xorg.xf86inputevdev
      pkgs.xorg.xkbcomp
      pkgs.xkeyboard_config
      pkgs.xorg.fontsunmisc
      pkgs.xorg.libxkbfile
      pkgs.xorg.xeyes
      # GNOME Games
      pkgs.gnome-mines
      pkgs.gnome-sudoku
      pkgs.five-or-more
      pkgs.tali
      pkgs.gnome-chess

      (pkgs.writeScriptBin "run_as_xfce" (builtins.readFile ./run_as_xfce.sh))
      pkgs.vim
      pkgs.busybox
      pkgs.util-linux
    ];

    system.nixos.distroName = "Asterinas";

    system.stateVersion = "25.05";
  };
}