{ pkgs }:

let
  systemdMinimal = pkgs.systemdMinimal.overrideAttrs (old: {
    src = pkgs.systemdMinimal.src;
    postInstall = ''
      ${old.postInstall or ""}
    '';
  });
in
systemdMinimal