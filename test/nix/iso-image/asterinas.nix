{ config, lib, pkgs, busybox, hostPlatform, ... }: {
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
    ];
  };
}
