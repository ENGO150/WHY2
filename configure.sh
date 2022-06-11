#!/bin/bash

# Get linux distro
DISTRO=$(lsb_release -is)

if [[ $(id -u) != "0" ]] && [[ $1 != "force" ]]; then
    echo "You must run this script as root! (To skip this, run with 'force' arg: \"./configure.sh force\")"
    exit 1
fi

# Get COMMAND
if [[ $DISTRO == "Arch" ]]; then
    COMMAND="pacman -S --needed --noconfirm gcc json-c curl make libgit2"
elif [[ $DISTRO == "Ubuntu" ]] || [[ $DISTRO == "Debian" ]]; then
    COMMAND="apt install -y gcc libjson-c-dev libcurl4-nss-dev make libgit2-dev"
else
    # 'Unsupported' distro
    echo "It seems you are using unsupported distribution... Don't worry, just install 'gcc', 'json-c', 'curl' and 'make' and you'll be fine."
    exit
fi

# Execute COMMAND
$COMMAND
