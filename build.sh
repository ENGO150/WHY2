#!/bin/sh

make

if [ "$1" == "debug" ]; then
    ./out/why2
fi