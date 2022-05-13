#!/bin/bash

####################

# Remove previous output
rm -rf out/*

# Variables
testFile="src/lib/test/main.c"
sourceFiles="src/lib/*.c"
includeFiles="include/*.h"

appFile="src/app/main.c"

compiler="cc"

testOutput="out/why2-test"
appOutput="out/why2-app"

installOutput="libwhy2.so"
includeDirectory="/usr/include/why2"

flags="-Wall -ljson-c -lcurl"

runTest()
{
    ###
    echo "Using '$compiler' as default compiler."
    ###

    # Check for debug flag
    if [[ "$1" == "debug" ]]; then ########## TEST & DEBUG ##########
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
}

runInstall()
{
    ###
    echo "Installing header files..."
    ###

    # Create why2 directory
    if [[ ! -d $includeDirectory ]]; then
        mkdir $includeDirectory
    fi

    cp $includeFiles $includeDirectory
    ln -sf $includeDirectory/why2.h /usr/include/why2.h

    if [[ "$1" == "include" ]]; then
        exit
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

    ###
    echo "Installing library..."
    ###

    mv $installOutput /usr/lib/

    # Compilation failed
    if [[ $? -ne 0 ]]; then
        echo -e "\nCompilation failed. Did you run 'configure.sh' first and 'build.sh' with sudo?" 
        exit 1
    fi

    ###
    echo "Finished! Cleaning up..."
    ###

    rm -rf *.o
}

runApp()
{
    ###
    echo "Compiling why2-app..."
    ###

    ###
    echo "Using '$compiler' as default compiler."
    ###

    # Check for debug flag
    if [[ "$1" == "debug" ]]; then ########## TEST & DEBUG ##########
        flags="$flags -g"
        echo "Using debug flag"
    fi

    flags="-lwhy2 $flags"

    ###
    echo "Compiling... (Flags: $flags)"
    ###

    # Compile
    $compiler $appFile $flags -o $appOutput

    # Compilation failed
    if [[ $? -ne 0 ]]; then
        echo -e "\nCompilation failed." 
        exit 1
    fi

    ###
    echo "Output generated as '$appOutput'"
    ###

    ###
    echo "Installing why2-app..."
    ###

    mv $appOutput /usr/bin/why2

    ###
    echo "Finished!"
    ###
}

if [[ "$1" == "test" ]]; then ########## TEST ##########
    runTest "$2"
elif [[ "$1" == "install" ]]; then ########## INSTALL ##########
    runInstall "$2"
elif [[ "$1" == "app" ]]; then ########## BUILD APP ##########
    runApp "$2"
else ########## ELSE ##########
    if [[ "$1" == "installTest" ]]; then ########## INSTALL & TEST ##########
        exit 69
        runInstall
        runTest
    else ########## ERR ##########
        ###
        echo "You have to enter 'install', 'test' or 'app' as arguments. (Or 'installTest' for install & test)"
        ###

        exit 1
    fi
fi