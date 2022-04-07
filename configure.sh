#!/bin/bash

# Get linux distro
DISTRO=$(lsb_release -is)

# Get COMMAND
if [[ $DISTRO == "Arch" ]]; then
    COMMAND="sudo pacman -S --needed json-c && sudo pacman -S --needed curl"
elif [[ $DISTRO == "Ubuntu" ]] || [[ $DISTRO == "Debian" ]]; then
    COMMAND="sudo apt install libjson-c3 && sudo apt install curl"
else
    # 'Unsupported' distro
    echo "It seems you are using unsupported distribution... Don't worry, just install https://github.com/json-c/json-c and you'll be fine."
fi

# Execute COMMAND
$COMMAND
