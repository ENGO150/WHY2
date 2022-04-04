#!/bin/sh

# Build project
make

# Check for 'debug' flag
if [ "$1" == "debug" ]; then
    ./out/why2
fi