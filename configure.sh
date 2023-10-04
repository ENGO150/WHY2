#!/bin/bash

# This is part of WHY2
# Copyright (C) 2022 Václav Šmejkal

# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.

# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.

# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.

# Get linux distro
DISTRO=$(lsb_release -is)

if [[ $(id -u) != "0" ]] && [[ $1 != "force" ]]; then
    echo "You must run this script as root! (To skip this, run with 'force' arg: \"./configure.sh force\")"
    exit 1
fi

COMMON="gcc make tmux"

# Get COMMAND
if [[ $DISTRO == "Arch" ]]; then
    COMMAND="pacman -S --needed --noconfirm $COMMON json-c curl libgit2 libyaml"
elif [[ $DISTRO == "Ubuntu" ]] || [[ $DISTRO == "Debian" ]]; then
    COMMAND="apt install -y $COMMON libjson-c-dev libcurl4-nss-dev libgit2-dev libyaml-dev"
elif [[ $DISTRO == "Gentoo" ]]; then
    COMMAND="emerge -vn gcc json-c curl make libgit2 tmux"

    # Add
    echo "LDPATH=/usr/lib/libwhy2.so" > /etc/env.d/99why2-core
    echo "LDPATH=/usr/lib/libwhy2-logger.so" > /etc/env.d/99why2-logger
    echo "LDPATH=/usr/lib/libwhy2-chat.so" > /etc/env.d/99why2-chat
    env-update && source /etc/profile
else
    # 'Unsupported' distro
    echo "It seems you are using unsupported distribution... Don't worry, just install 'gcc', 'json-c', 'curl', 'libgit2', 'tmux', 'libyaml' and 'make' and you'll be fine."
    exit
fi

# Execute COMMAND
$COMMAND
