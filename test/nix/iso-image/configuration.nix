{ config, lib, pkgs, ... }:
let
  runAsXfce = pkgs.writeScriptBin "run_as_xfce" (builtins.readFile ./run_as_xfce.sh);
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

    nixpkgs.overlays = [
      (import ./overlay.nix)
    ];
    services.xserver.enable = true;
    services.xserver.desktopManager.xfce.enable = true;

    systemd.package = pkgs.callPackage ./systemd.nix { };
    systemd.coredump.enable = false;
    systemd.services.systemd-tmpfiles-setup.enable = false;
    services.timesyncd.enable = false;
    systemd.oomd.enable = false;
    services.udev.enable = false;
    systemd.services."xfce-desktop" = {
      description = "XFCE Desktop Environment";
      after = [ "multi-user.target" ];
      wantedBy = [ "graphical.target" ];
      serviceConfig = {
        Environment = "DISPLAY=:0";
        ExecStart = "${runAsXfce}/bin/run_as_xfce";
        StandardOutput = "tty";
        StandardError = "tty";
        KillMode = "process";
        Delegate = "yes";
        Restart = "no";
        Type = "simple";
      };
    };
    networking.dhcpcd.enable = false;
    systemd.services."systemd-random-seed".enable = false;
    systemd.services."resolvconf".serviceConfig = {
      ExecStart = lib.mkForce "/bin/true";
    };
    systemd.services."network-setup".serviceConfig = {
      ExecStart = lib.mkForce "/bin/true";
    };

    environment.systemPackages = [
      pkgs.xorg.xf86videofbdev
      runAsXfce
      pkgs.vim
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
