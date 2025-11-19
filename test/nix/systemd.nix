{ pkgs }:

let
  systemdMinimal = pkgs.systemdMinimal.overrideAttrs (old: {
    src = pkgs.systemdMinimal.src;
    postInstall = ''
      ${old.postInstall or ""}

      mkdir -p "$out/example/systemd/system"
      cat > "$out/example/systemd/system/systemd-logind.service" <<'EOF'
# placeholder for $out
[Unit]
Description=systemd-logind (placeholder)
EOF

      cat > "$out/example/systemd/system/systemd-user-sessions.service" <<'EOF'
# placeholder injected by override
[Unit]
Description=placeholder systemd-user-sessions (disabled)
EOF

      cat > "$out/example/systemd/system/dbus-org.freedesktop.login1.service" <<'EOF'
# placeholder for $out
[Unit]
Description=placeholder dbus-org.freedesktop.login1.service
[Service]
Type=dbus
BusName=org.freedesktop.login1
ExecStart=/bin/true
EOF

      cat > "$out/example/systemd/system/user@.service" <<'EOF'
# placeholder for $out
[Unit]
Description=placeholder user@.service
[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/true
EOF

      cat > "$out/example/systemd/system/user-runtime-dir@.service" <<'EOF'
# placeholder for $out
[Unit]
Description=placeholder user-runtime-dir@.service
[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/mkdir -p /run/user/%i
EOF

      cat > "$out/example/systemd/system/local-fs.target.wants/tmp.mount" <<'EOF'
# placeholder for $out
# This file is intentionally empty as a placeholder for tmp.mount
EOF

      cat > "$out/example/systemd/system/systemd-firstboot.service" <<'EOF'
# placeholder for $out
[Unit]
Description=placeholder systemd-firstboot
[Service]
Type=oneshot
ExecStart=/bin/true
EOF

      cat > "$out/example/systemd/system/systemd-random-seed.service" <<'EOF'
# placeholder for $out
[Unit]
Description=placeholder systemd-random-seed
[Service]
Type=oneshot
ExecStart=/bin/true
EOF

      mkdir -p "$out/example/systemd/system/getty@.service.d"
      cat > "$out/example/systemd/system/getty@.service.d/override.conf" <<'EOF'
[Service]
Restart=no
EOF

    '';
  });
in
systemdMinimal