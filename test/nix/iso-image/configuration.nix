{ config, lib, pkgs, ... }: {
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

    systemd.enableCgroupAccounting = false;

    environment.defaultPackages = [ ];

    system.nixos.distroName = "Asterinas";

    system.stateVersion = "25.05";
  };
}
