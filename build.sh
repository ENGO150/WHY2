#!/bin/bash

####################

# Remove previous output
rm -rf out/*

# Variables
testFile="src/lib/test/main.c"
sourceFiles="src/lib/*.c"
includeFiles="include/*.h"

compiler="cc"
testOutput="out/why2-test"
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

    flags="-lwhy2 $flags"

    ###
    echo "Compiling... (Flags: $flags)"
    ###

    # Compile
    $compiler $testFile $flags -o $testOutput

    # Compilation failed
    if [[ $? -ne 0 ]]; then
        echo -e "\nCompilation failed. Did you run 'configure.sh' and 'build.sh install' first?" 
        exit 1
    fi

    ###
    echo "Output generated as '$testOutput'"
    ###
elif [[ "$1" == "install" ]]; then ########## INSTALL ##########
    if [[ "$2" != "lib" ]]; then
        ###
        echo "Installing header files..."
        ###

        # Create why2 directory
        if [[ ! -d $includeDirectory ]]; then
            mkdir $includeDirectory
        fi

        cp $includeFiles $includeDirectory
    fi

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

    if [[ "$2" != "lib" ]]; then
        ###
        echo "Installing library..."
        ###

        mv $installOutput /usr/lib/

        # Compilation failed
        if [[ $? -ne 0 ]]; then
            echo -e "\nCompilation failed. Did you run 'configure.sh' first and 'build.sh' with sudo?" 
            exit 1
        fi
    fi

    ###
    echo "Finished! Cleaning up..."
    ###

    rm -rf *.o
else ########## ELSE ##########
    if [[ "$1" == "installTest" ]]; then ########## INSTALL & TEST ##########
        ./build.sh install
        ./build.sh test
    else ########## ERR ##########
        ###
        echo "You have to enter 'test' or 'install' as arguments."
        ###

        exit 1
    fi
fi