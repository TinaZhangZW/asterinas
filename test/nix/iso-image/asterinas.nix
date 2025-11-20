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
    asterinas.overlay = lib.mkOption {
      type = lib.types.path;
      default = ./overlay.nix;
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
          cp -L ${config.asterinas.overlay} $out/overlay.nix
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

      pkgs.vim
    ];

    services.xserver.enable = false;
    services.xserver.desktopManager.xfce.enable = false;
  };
}
