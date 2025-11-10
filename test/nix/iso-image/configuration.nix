{ config, lib, pkgs, ... }:
let
  xfce = import ./xfce.nix { inherit pkgs; };
  xorg = import ./xorg.nix { inherit pkgs; };
in
{
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
        "PATH=/bin:/nix/var/nix/profiles/system/sw/bin ostd.log_level=error -- sh /init root=/dev/vda2 init=/run/current-system/systemd/lib/systemd/systemd rd.break=0";
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

    i18n.defaultLocale = "en_US.UTF-8";
    i18n.supportedLocales = [ "en_US.UTF-8/UTF-8" ];
    environment.variables.LANG = "en_US.UTF-8";

    systemd.defaultUnit = "multi-user.target";
    systemd.package = pkgs.callPackage ./systemd.nix { };
    systemd.coredump.enable = false;
    services.timesyncd.enable = false;
    systemd.oomd.enable = false;
    services.udev.enable = false;
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

    systemd.services.systemd-random-seed = {
      enable = false;
    };

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
      pkgs.bash
    ];

    services.getty.autologinUser = "root";
    users.users.root = {
      shell = "${pkgs.bash}/bin/bash";
      hashedPassword = null;
    };

    system.nixos.distroName = "Asterinas";

    system.stateVersion = "25.05";
  };
}