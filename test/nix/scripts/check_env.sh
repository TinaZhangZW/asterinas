#!/usr/bin/env bash
# ...existing code...
# Collect share dirs from /nix/store and export XDG_DATA_DIRS

# Build XDG_DATA_DIRS by starting from sensible defaults and appending /nix/store/*/share
paths="${XDG_DATA_DIRS:-/run/current-system/sw/share:/usr/share:/usr/local/share}"
if [ -d /nix/store ]; then
  tmp_share="$(mktemp)" || exit 1
  find /nix/store -maxdepth 2 -type d -name share 2>/dev/null | sort -u > "$tmp_share"
  while IFS= read -r dir; do
    [ -z "$dir" ] && continue
    [ -d "$dir" ] || continue
    case ":$paths:" in
      *":$dir:"*) ;;
      *) paths="$paths:$dir" ;;
    esac
  done < "$tmp_share"
  rm -f "$tmp_share"
fi
export XDG_DATA_DIRS="$paths"
printf '%s\n' "$XDG_DATA_DIRS"

# Build GIO_EXTRA_MODULES by searching for lib/gio/modules under /nix/store
gio_paths="${GIO_EXTRA_MODULES:-/run/current-system/sw/lib/gio/modules:/usr/lib/gio/modules}"
if [ -d /nix/store ]; then
  tmp_gio="$(mktemp)" || exit 1
  find /nix/store -type d -path '*/lib/gio/modules' 2>/dev/null | sort -u > "$tmp_gio"
  while IFS= read -r gdir; do
    [ -z "$gdir" ] && continue
    [ -d "$gdir" ] || continue
    case ":$gio_paths:" in
      *":$gdir:"*) ;;
      *) gio_paths="$gio_paths:$gdir" ;;
    esac
  done < "$tmp_gio"
  rm -f "$tmp_gio"
fi
export GIO_EXTRA_MODULES="$gio_paths"
printf '%s\n' "$GIO_EXTRA_MODULES"

config_paths="${XDG_CONFIG_DIRS:-/etc/xdg:/run/current-system/sw/etc/xdg}"
if [ -d /nix/store ]; then
  tmp_xdg="$(mktemp)" || exit 1
  find /nix/store -type d \( -path '*/etc/xdg' -o -path '*/share/xdg' \) 2>/dev/null | sort -u > "$tmp_xdg"
  while IFS= read -r cdir; do
    [ -z "$cdir" ] && continue
    [ -d "$cdir" ] || continue
    case ":$config_paths:" in
      *":$cdir:"*) ;;
      *) config_paths="$config_paths:$cdir" ;;
    esac
  done < "$tmp_xdg"
  rm -f "$tmp_xdg"
fi
export XDG_CONFIG_DIRS="$config_paths"
printf '%s\n' "$XDG_CONFIG_DIRS"
