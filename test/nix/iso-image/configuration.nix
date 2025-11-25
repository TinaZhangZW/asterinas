{ config, lib, pkgs, ... }: {
  imports = [ ./overlays/default.nix ];

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
        "ostd.log_level=error -- sh /init root=/dev/vda2 init=STAGE_2_INIT rd.break=0";
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
      STAGE_2_INIT=$(readlink -f /run/current-system/stage-2-init)
      sed -i "s|STAGE_2_INIT|$STAGE_2_INIT|g" /boot/grub/grub.cfg
    '';
    boot.postBootCommands = ''
      echo "Executing postBootCommands..."
      echo "nameserver 30.64.127.127" > /etc/resolv.conf
      PATH=$PATH:/nix/var/nix/profiles/system/sw/bin:~/.nix-profile/bin
      ${pkgs.bash}/bin/sh
    '';
    system.systemBuilderCommands = ''
      echo ${config.asterinas.kernel-params} > $out/kernel-params
      mv $out/init $out/stage-2-init
      sed -i '102d' $out/stage-2-init
      ln -s /bin/busybox $out/init
      ln -s ${config.asterinas.kernel} $out/kernel
      ln -s ${config.asterinas.initramfs}/initrd $out/initrd
    '';

    nix.settings = {
      filter-syscalls = false;
      require-sigs = false;
      sandbox = false;
    };

    systemd.enableCgroupAccounting = false;

    environment.defaultPackages = [ pkgs.hello ];

    system.nixos.distroName = "Asterinas";

    system.stateVersion = "25.05";
  };
}
