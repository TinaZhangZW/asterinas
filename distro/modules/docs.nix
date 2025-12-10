{ config, lib, pkgs, ... }:

{
  options.asterinas.docs.enable = lib.mkEnableOption "Install Asterinas documentation into the user's home directory";

  config = lib.mkIf config.asterinas.docs.enable {
    system.activationScripts.copyDocs = {
      text = ''
        TARGET_USER="root"
        HOME_DIR=$(getent passwd "$TARGET_USER" | cut -d: -f6)

        if [ -d "$HOME_DIR" ]; then
          echo "Installing Asterinas documentation to $HOME_DIR/Documents..."
          DOCS_DEST="$HOME_DIR/Documents"
          mkdir -p "$DOCS_DEST"
          cp -r ${pkgs.asterinas-docs}/* "$DOCS_DEST/"
          chown -R "$TARGET_USER" "$DOCS_DEST"
        fi
      '';
      # This ensures the script runs after user accounts have been created.
      deps = [ "users" ];
    };
  };
}