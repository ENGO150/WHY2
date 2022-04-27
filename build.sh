#!/bin/bash

####################

# Remove previous output & versions.json
rm -rf out/*
rm -f versions.json

# Variables
files="
      src/test/main.c

      src/*.c
      include/*.h
      "

compiler="cc"
output="out/why2"
flags="-ljson-c -lcurl"

# Check for debug flag
if [ "$1" == "debug" ]; then
    flags="$flags -g"
    echo "Using debug flag"
fi

###
echo "Using '$compiler' as default compiler. (Flags: $flags)"
echo "Compiling..."
###

# Compile
$compiler $files $flags -o $output

###
echo "Output generated as '$output'"
###


