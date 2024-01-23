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
ARCH_GENTOO_COMMON="$COMMON json-c curl libgit2"

# Get COMMAND
if [[ $DISTRO == "Arch" ]]; then
    COMMAND="pacman -S --needed --noconfirm $ARCH_GENTOO_COMMON libyaml rustup"

    COMMAND="$COMMAND && pacman -S --needed --noconfirm cargo"
elif [[ $DISTRO == "Ubuntu" ]] || [[ $DISTRO == "Debian" ]]; then
    COMMAND="apt install -y $COMMON libjson-c-dev libcurl4-nss-dev libgit2-dev libyaml-dev rustup"

    COMMAND="$COMMAND && apt install -y cargo"
elif [[ $DISTRO == "Gentoo" ]]; then
    COMMAND="emerge -vn $ARCH_GENTOO_COMMON dev-libs/libyaml"

    # Add
    echo "LDPATH=/usr/lib/libwhy2.so" > /etc/env.d/99why2-core
    echo "LDPATH=/usr/lib/libwhy2-logger.so" > /etc/env.d/99why2-logger
    echo "LDPATH=/usr/lib/libwhy2-chat.so" > /etc/env.d/99why2-chat
    env-update && source /etc/profile

    # Install Rust
    if ! [ -x "$(command -v cargo)" ]; then
        curl https://sh.rustup.rs -sSf | sh # Install Rust and components
        source "$HOME/.cargo/env"
    fi
else
    # 'Unsupported' distro
    echo "It seems you are using unsupported distribution... Don't worry, just install 'gcc', 'json-c', 'curl', 'libgit2', 'tmux', 'libyaml' and 'make' and you'll be fine."
    exit
fi

# Execute COMMAND
$COMMAND

# Config Rust version
rustup default stable