#!/bin/sh

# SPDX-License-Identifier: MPL-2.0

source /etc/profile

# Settings for lbreakout2
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/0}"
if [ ! -d "$XDG_RUNTIME_DIR" ]; then
  mkdir -p "$XDG_RUNTIME_DIR"
  chmod 700 "$XDG_RUNTIME_DIR"
fi

export SDL_VIDEODRIVER="${SDL_VIDEODRIVER:-x11}"
export SDL_RENDER_DRIVER="${SDL_RENDER_DRIVER:-software}"
export SDL_AUDIODRIVER="${SDL_AUDIODRIVER:-dummy}"
export ALSOFT_DRIVERS="${ALSOFT_DRIVERS:-null}"

mkdir -p /root/.lgames
cat > /root/.lgames/lbreakout2.conf <<'EOF'
@
set_id_home�0
set_count_home�1
player_count�1
player0�Michael
player1�Mr.X
player2�Mr.Y
player3�Mr.Z
diff�2
starting_level�0
rel_warp_limit�80
add_bonus_levels�1
left�276
right�275
fire_left�121
fire_right�32
return�8
turbo�120
rel_motion�1
grab�1
motion_mod�120
convex�1
linear_corner�0
random_angle�1
maxballspeed�900
invert�0
sound�1
volume�8
speech�1
badspeech�0
audio_buffer_size�512
anim�2
fullscreen�0
fade�1
bonus_info�1
fps�0
ball_level�0
debris_level�1
i_key_speed�500
use_hints�1
return_on_click�0
theme_id�0
theme_count�4
server�217.160.141.22:8000
local_port�8001
username�player
mp_diff�1
mp_rounds�1
mp_frags�10
mp_balls�3
EOF

# Settings for short-cuts
mkdir -p /root/Desktop
cp /nix/store/1b8bwn4zs94q73yk9ihgcr9spcz5pj55-lbreakout2-2.6.5/share/applications/lbreakout2.desktop /root/Desktop/
cp /nix/store/njsyv1ziyvyqj66q2m2i66b9w28fdcah-xboard-4.9.1/share/applications/xboard.desktop /root/Desktop/
cp /nix/store/1r4y3qnaz0s68r2jp39hbx8k8rhgdisn-gnome-mines-48.1/share/applications/org.gnome.Mines.desktop /root/Desktop/

# Step 1: run dbus
mkdir -p /var/lib/dbus /usr/share/X11/xorg.conf.d
[ -f /var/lib/dbus/machine-id ] || dbus-uuidgen --ensure=/var/lib/dbus/machine-id

if command -v dbus-launch >/dev/null 2>&1; then
  eval "$(dbus-launch --sh-syntax)"
fi

# Step 2: run Xorg
XKB_DATA="/run/current-system/sw/share/X11/xkb"
MODULE_PATH="/run/current-system/sw/lib/xorg/modules"

nohup Xorg :0 \
  -modulepath "$MODULE_PATH" \
  -xkbdir "$XKB_DATA" \
  -logverbose 0 \
  -logfile /var/log/xorg_debug.log \
  -novtswitch \
  -keeptty \
  -keyboard keyboard \
  -pointer mouse0 \
  > /var/log/xorg.log 2>&1 &


# Step 3: run xfce4
export DISPLAY=:0
LOG=/var/log/xfce-session.log
mkdir -p "$(dirname "$LOG")"
: > "$LOG"                 # truncate/create
chmod 600 "$LOG"
nohup xfce4-session >>"$LOG" 2>&1 &
