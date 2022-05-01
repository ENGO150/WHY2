#!/bin/bash

####################

# Remove previous output
rm -rf out/*

# Variables
testFile="src/test/main.c"
sourceFiles="src/*.c"
includeFiles="include/*.h"

compiler="cc"
output="out/why2"
installOutput="libwhy2.so"
flags="-Wall -ljson-c -lcurl"
includeDirectory="/usr/include/why2"

if [[ "$1" == "test" ]]; then ########## TEST ##########
    ###
    echo "Using '$compiler' as default compiler."
    ###

    # Check for debug flag
    if [[ "$2" == "debug" ]]; then ########## TEST & DEBUG ##########
        flags="$flags -g"
        echo "Using debug flag"
    fi

    ###
    echo "Compiling... (Flags: $flags)"
    ###

    # Compile
    $compiler -lwhy2 $testFile $flags -o $output

    # Compilation failed
    if [[ $? -ne 0 ]]; then
        echo -e "\nCompilation failed. Did you run 'configure.sh' and 'build.sh install' first?" 
        exit 1
    fi

    ###
    echo "Output generated as '$output'"
    ###
elif [[ "$1" == "install" ]]; then ########## INSTALL ##########
    ###
    echo "Installing header files..."
    ###

    # Create why2 directory
    if [[ ! -d $includeDirectory ]]; then
        mkdir $includeDirectory
    fi
    
    cp $includeFiles $includeDirectory

    ###
    echo "Using '$compiler' as default compiler."
    ###

    flags="-Wall -fPIC -c"

    ###
    echo "Compiling... (Flags: $flags)"
    ###

    $compiler $flags $sourceFiles

    flags="-Wall -shared"

    ###
    echo "Compiling library... (Flags: $flags)"
    ###

    $compiler $flags -o $installOutput *.o

    ###
    echo "Installing library..."
    ###

    mv $installOutput /usr/lib/

    # Compilation failed
    if [[ $? -ne 0 ]]; then
        echo -e "\nCompilation failed. Did you run 'build.sh' with sudo?" 
        exit 1
    fi

    ###
    echo "Finished! Cleaning up..."
    ###

    rm -rf *.o
else ########## ERR ##########
    ###
    echo "You have to enter 'test' or 'install' as arguments."
    ###

    exit 1
fi