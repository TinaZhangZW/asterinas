{ config, lib, pkgs, ... }:

let
  xfce-desktop-config = pkgs.writeText "xfce4-desktop.xml" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <channel name="xfce4-desktop" version="1.0">
      <property name="last-settings-migration-version" type="uint" value="1"/>
      <property name="backdrop" type="empty">
        <property name="screen0" type="empty">
          <property name="monitordefault" type="empty">
            <property name="workspace0" type="empty">
              <property name="last-image" type="string" value="${pkgs.asterinas-docs}/Desktop_Background.jpg"/>
              <property name="image-style" type="int" value="5"/> <!-- 5 = Zoomed -->
            </property>
          </property>
        </property>
      </property>
      <property name="desktop-icons" type="empty">
        <property name="file-icons" type="empty">
          <property name="show-home" type="bool" value="true"/>
          <property name="show-filesystem" type="bool" value="true"/>
          <property name="show-removable" type="bool" value="true"/>
          <property name="show-trash" type="bool" value="true"/>
        </property>
        <property name="icon-size" type="uint" value="48"/>
      </property>
      <property name="last" type="empty">
        <property name="window-width" type="int" value="708"/>
        <property name="window-height" type="int" value="547"/>
      </property>
    </channel>
  '';
in
{
  options.asterinas.wallpaper.enable = lib.mkEnableOption "Set default Asterinas wallpaper for XFCE";

  config = lib.mkIf config.asterinas.wallpaper.enable {
    environment.etc."xdg/xfce4/xfconf/xfce-perchannel-xml/xfce4-desktop.xml" = {
      source = xfce-desktop-config;
      mode = "0644";
    };
  };
}