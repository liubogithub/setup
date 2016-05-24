#!/bin/bash

XLABEL=""
YLABEL=""

gdm_config()
{
	XLABLE="fio -direct=1 -bsrange=4k-4k -size=512m"

	if [ "x$1" == "xaggrb" ]; then
		YLABLE="KB/s"
	elif [ "x$1" == "xclat95" ]; then
		YLABLE="usec"
	fi
}

gdm_create()
{
	gdm_config $2
	# eg. seqw-aggrb.gdm
	cat > ${1}-${2}.gdm <<EOF
set title "${1}-${2}";
set style data histogram;
set style histogram clustered gap 3;
set boxwidth 0.8 relative;
set style fill solid;
set ylabel "$YLABLE"
set xlabel "$XLABLE" 
plot for [dataset in "btrfs ext4 xfs ocfs2mail ocfs2datafiles ocfs2vmstore btrfsnodatacow btrfscompress"] "results/".dataset."-${1}-${2}.dat" u 1:xtic(2) t dataset
EOF
}

# aggrb clat95
for i in "seqw" "randw" "seqr" "randr"; do
	for j in "aggrb" "clat95"; do
		gdm_create $i $j
		gnuplot -p -c ${i}-${j}.gdm
	done
done
