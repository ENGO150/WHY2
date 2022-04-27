#!/bin/bash

# Get linux distro
DISTRO=$(lsb_release -is)

if [[ $(id -u) != "0" ]] && [[ $1 != "force" ]]; then
    echo "You must run this script as root! (To skip this, run with 'force' arg: \"./configure.sh force\")"
    exit 1
fi

# Get COMMAND
if [[ $DISTRO == "Arch" ]]; then
    COMMAND="pacman -S --needed json-c curl"
elif [[ $DISTRO == "Ubuntu" ]] || [[ $DISTRO == "Debian" ]]; then
    COMMAND="apt install libjson-c3 curl"
else
    # 'Unsupported' distro
    echo "It seems you are using unsupported distribution... Don't worry, just install https://github.com/json-c/json-c (+ CURL if you haven't installed it already) and you'll be fine."
    exit
fi

# Execute COMMAND
$COMMAND
