#!/bin/sh

echo "Starting stage2.sh"

if [ -t 0 ]; then
    while read -t 0 -n 1 _ >/dev/null 2>&1; do :; done
fi
for dev in /dev/tty /dev/console /dev/ttyS0 /dev/hvc0; do
    [ -e "$dev" ] || continue
    echo "flush input buffer for $dev"
    while read -t 0 -n 1 _ < "$dev" >/dev/null 2>&1; do :; done
done

sleep 0.02 2>/dev/null || true

exec /run/current-system/systemd/lib/systemd/systemd
