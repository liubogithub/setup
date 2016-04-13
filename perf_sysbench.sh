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
ulimit -n 10240
umount $M $D

if [ "x$1" = "xbtrfs" ]; then
	mkfs.btrfs -f $D || exit
elif [ "x$1" = "xxfs" ]; then
	mkfs.xfs -f $D || exit
elif [ "x$1" = "xext4" ]; then
	mkfs.ext4 -F -q $D || exit
fi

mount $D $M || exit

echo "perf start..."
cd $M
id="`date +%d%m%y_%H%M%S`"

$tsb --num-threads=16 --test=fileio --file-total-size=4G --file-test-mode=rndrw prepare

# random rw use 8K to match DB workload
$tsb --num-threads=16 --test=fileio --file-block-size=8K --file-total-size=4G --file-test-mode=rndrw run > /tmp/perf_sysbench_${1}_${id}.log 2>&1

# sequential rw use 1M to match DB workload
#$tsb --num-threads=16 --test=fileio --file-block-size=8K --file-total-size=4G --file-test-mode=rndrw run > /tmp/perf_sysbench_${id}.log 2>&1

$tsb --num-threads=16 --test=fileio --file-total-size=4G --file-test-mode=rndrw cleanup
cd -
echo "check /tmp/perf_sysbench_${1}_${id}.log..."
