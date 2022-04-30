#!/bin/bash

####################

# Remove previous output
rm -rf out/*

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
if [[ "$*" == "debug" ]]; then
    flags="$flags -g"
    echo "Using debug flag"
fi

###
echo "Using '$compiler' as default compiler. (Flags: $flags)"
echo "Compiling..."
###

# Compile
$compiler $files $flags -o $output

if [[ $? -ne 0 ]]; then
    echo -e "\nCompilation failed. Did you run 'configure.sh' first?" 
    exit 1
fi

###
echo "Output generated as '$output'"
###


