#!/bin/bash

if [ "x$1" = "xdev" ]; then
    ssh b.liu@11.238.158.165
elif [ "x$1" = "xtmux" ]; then
    ssh -t b.liu@11.238.158.165 byobu attach
else
    echo "no box is registered"
    exit 1
fi
