{ config, lib, pkgs, busybox, hostPlatform, ... }:
let
  xfce = import ../xfce.nix { inherit pkgs; };
  xorg = import ../xorg.nix { inherit pkgs; };
in
{
  options = {
    asterinas.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };
    asterinas.kernel = lib.mkOption {
      type = lib.types.path;
      # Note: The kernel should be built with `BOOT_PROTOCOL=linux-efi-handover64`.
      default = ../../../target/osdk/iso_root/boot/aster-nix-osdk-bin;
    };
    asterinas.initramfs-init = lib.mkOption {
      type = lib.types.path;
      default = ./initramfs-init.sh;
    };
    asterinas.configuration = lib.mkOption {
      type = lib.types.path;
      default = ./configuration.nix;
    };
    asterinas.splash = lib.mkOption {
      type = lib.types.path;
      default = ./splash.png;
    };
    asterinas.xorg = lib.mkOption {
      type = lib.types.path;
      default = ../xorg.nix;
    };
    asterinas.xfce = lib.mkOption {
      type = lib.types.path;
      default = ../xfce.nix;
    };
    asterinas.systemd = lib.mkOption {
      type = lib.types.path;
      default = ../systemd.nix;
    };
    asterinas.package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.stdenv.mkDerivation {
        pname = "asterinas";
        version = "0.1.0";
        buildCommand = ''
          mkdir -p $out
          cp -L ${config.asterinas.kernel} $out/kernel
          cp -L ${config.asterinas.initramfs-init} $out/initramfs-init.sh
          cp -L ${config.asterinas.configuration} $out/configuration.nix
          cp -L ${config.asterinas.splash} $out/splash.png
          cp -L ${config.asterinas.xorg} $out/xorg.nix
          cp -L ${config.asterinas.xfce} $out/xfce.nix
          cp -L ${config.asterinas.systemd} $out/systemd.nix
          if [ -e ${../patches} ]; then
            mkdir -p $out/patches
            cp -r ${../patches}/* $out/patches/
          fi
          if [ -e ${../scripts/run_as_xfce.sh} ]; then
            cp ${../scripts}/run_as_xfce.sh $out/run_as_xfce.sh
          fi
        '';
      };
    };
  };

  config = lib.mkIf config.asterinas.enable {
    system.activationScripts.asterinas.text = ''
      ln -sf ${config.asterinas.package} /asterinas
    '';
    environment.systemPackages = [
      (pkgs.writeScriptBin "install_asterinas"
        (builtins.readFile ./install-asterinas.sh))
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
      pkgs.xfce.exo
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

      pkgs.vim
      pkgs.busybox
      pkgs.util-linux
    ];

    services.xserver.enable = false;
    services.xserver.desktopManager.xfce.enable = false;
  };
}
