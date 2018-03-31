#!/bin/bash

_fail()
{
    echo $@
    exit
}

D=/dev/sda

umount $D
umount /mnt/btrfs /mnt/scratch

mkfs.btrfs -q -f $D || _fail "mkfs"

./check -s btrfs $@
