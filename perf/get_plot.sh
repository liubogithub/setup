#!/bin/bash

rwtype=(seqw randw seqr randr)
fstype=(btrfs ext4 xfs btrfsnodatacow btrfscompress ocfs2mail ocfs2datafiles ocfs2vmstore)

echo -e "thread=1\nthread=2\nthread=4\nthread=8" > /tmp/plot.pad

for ((j=0; j<8; j++)); do

	#------------------------------------
	# the 1st awk
	# (NR - 1) % 4 == i
	# seq write:  0
	# rand write: 1
	# seq read:   2
	# rand read:  3
	#------------------------------------
	# the 2nd awk: get average value
	# num/5: ind[1,5]
	#------------------------------------
	# results: 
	# for i in aggrb clat95 cpuusr cpusys cpuctx; do
	# 	thread1_num
	# 	thread2_num
	# 	thread4_num
	# 	thread8_num
	# done

	FS=${fstype[j]}
	mkdir -p results
	for i in `seq 0 3`; do
		for prog in "aggrb" "clat95" "cpuusr" "cpusys" "cpuctx"; do
			./get_all.sh $FS ./get_${prog}.sh | awk -v rw=$i '{if ((NR - 1) % 4 == rw) print $0 }' | awk '{if ((NR - 1) % 5 == 4) {num+=$0; print num/5; num=0;} else num+=$0}' > results/${FS}-${rwtype[i]}-${prog}.dat

			paste "results/${FS}-${rwtype[i]}-${prog}.dat" "/tmp/plot.pad" | column -t >> /tmp/plot.dat

			mv /tmp/plot.dat results/${FS}-${rwtype[i]}-${prog}.dat
		done
	done
done

rm -f /tmp/plot.pad
