#!/bin/sh
set -eu

# require root
if [ "$(id -u)" != "0" ]; then
  printf 'error: must run as root\n' >&2
  exit 1
fi

if [ "$#" -ne 1 ]; then
  printf 'usage: %s <target>\nvalid: graphical.target multi-user.target\n' "$0" >&2
  exit 2
fi

TARGET="$1"
case "$TARGET" in
  graphical.target|multi-user.target) ;;
  *)
    printf 'error: invalid target: %s\n' "$TARGET" >&2
    exit 3
    ;;
esac

DEF_LINK="/etc/systemd/system/default.target"

if [ -L "$DEF_LINK" ]; then
  CURRENT_TARGET=$(basename "$(readlink -- "$DEF_LINK")")
  if [ "$CURRENT_TARGET" = "$TARGET" ]; then
    printf 'Default target is already set to %s.\n' "$TARGET"
    exit 0
  fi
fi

# 1) resolve the real path that /etc/systemd/system/ points to
REAL_DIR=$(readlink -f "/etc/systemd/system" 2>/dev/null || true)
if [ -z "$REAL_DIR" ]; then
  printf 'error: cannot resolve %s\n' "$REAL_DIR" >&2
  exit 4
fi

OLD_PWD=$(pwd)
trap 'cd "$OLD_PWD" >/dev/null 2>&1 || true' EXIT

# 2) cd to that folder
cd "$REAL_DIR"

# 3) remove default.target in that folder
rm -f default.target

# 4) create a symlink named default.target pointing to the chosen target file (relative link)
#    The target file (graphical.target or multi-user.target) is expected to exist in this folder.
if ln -s "$TARGET" default.target; then
  printf 'created %s/default.target -> %s\n' "$REAL_DIR" "$TARGET"
else
  printf 'error: failed to create %s/default.target (readonly filesystem?)\n' "$STORE_DIR" >&2
  exit 7
fi

# 5) cd back (handled by trap)
printf 'done\n'
exit 0
