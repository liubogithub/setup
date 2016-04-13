#!/bin/bash

if [ ! -x `which sysbench` ]; then
	echo "sysbench is not installed..."
	exit
fi

#test of sysbench
tsb=`which sysbench`

M=/mnt/btrfs
D=/dev/sdf

echo "prepare..."
umount $M $D

mkfs.btrfs -f $D || exit
mount $D $M || exit

echo "perf start..."
cd $M
id=$((RANDOM % 1000))

$tsb --num-threads=16 --test=fileio --file-total-size=4G --file-test-mode=rndrw prepare
$tsb --num-threads=16 --test=fileio --file-block-size=4K --file-total-size=4G --file-test-mode=rndrw run > /tmp/perf_sysbench_${id}.log 2>&1
$tsb --num-threads=16 --test=fileio --file-total-size=4G --file-test-mode=rndrw cleanup
cd -
echo "check /tmp/perf_sysbench_${id}.log..."
