{ pkgs, ... }:
let
  configuration = {
    imports = [
      "${pkgs.path}/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix"
      "${pkgs.path}/nixos/modules/installer/cd-dvd/channel.nix"
      ./asterinas.nix
    ];
  };
in (pkgs.nixos configuration).config.system.build.isoImage
