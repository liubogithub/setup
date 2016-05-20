#!/bin/bash

DEV=/dev/sdb1
MNT=/mnt/test

FIO=`which fio`
# job template
JTP="/home/boliu/uek_perf/tiobench-example.fio"

LOG="/home/boliu/uek_perf/log/`date +%y%m%d_%H%M%S`"

mkdir -p $LOG

# i: numjobs (num_threads)
for THREAD in 1 2 4 8; do

	# create job file first
	JOB="${LOG}/tiobench-thread-${THREAD}.fio"
	sed -e "s/numjobs=4/numjobs=$THREAD/g" $JTP > $JOB

	for FS in "btrfs" "ext4" "xfs" "btrfsnodatacow" "btrfscompress" "ocfs2mail" "ocfs2datafiles" "ocfs2vmstore"; do
		# start mark
		echo "$FS..............num_threads=$THREAD"

		mkdir -p $MNT

		MNTOPT=""
		MKFS=""
		if [ "x$FS" = "xbtrfs" ]; then
			MKOPT="-f -q"
			MKFS="mkfs.btrfs"
		elif [ "x$FS" = "xbtrfsnodatacow" ]; then
			MKOPT="-f -q"
			MNTOPT="-o nodatacow"
			MKFS="mkfs.btrfs"
		elif [ "x$FS" = "xbtrfscompress" ]; then
			MKOPT="-f -q"
			MNTOPT="-o compress"
			MKFS="mkfs.btrfs"
		elif [ "x$FS" = "xext4" ]; then
			MKOPT="-F -q"
			MKFS="mkfs.ext4"
		elif [ "x$FS" = "xxfs" ]; then
			MKOPT="-f -q"
			MKFS="mkfs.xfs"
		elif [ "x$FS" = "xocfs2datafiles" ]; then
			MKOPT="-F -q -M local -T datafiles"
			MKFS="mkfs.ocfs2"
		elif [ "x$FS" = "xocfs2mail" ]; then
			MKOPT="-F -q -M local -T mail"
			MKFS="mkfs.ocfs2"
		elif [ "x$FS" = "xocfs2vmstore" ]; then
			MKOPT="-F -q -M local -T vmstore"
			MKFS="mkfs.ocfs2"
		else
			echo "unsupported fs"
			exit 1
		fi

		echo "$MKFS $MKOPT $DEV; mount $DEV $MNT $MNTOPT" >> ${LOG}/${FS}.setup

		# run each FS 5 times in order to get average number 
		for ((ind = 1; ind < 6; ind++)); do

			umount $MNT $DEV > /dev/null 2>&1

			echo "y" | $MKFS $MKOPT $DEV || exit

			mount $DEV $MNT $MNTOPT || exit

			$FIO --directory=$MNT $JOB > ${LOG}/${FS}-thread${THREAD}-ind${ind}.log
		done
	done
done
