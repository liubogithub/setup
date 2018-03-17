#!/bin/bash

TESTPATH="$1"
FWDPORT="$2"
if [ ! -e $TESTPATH ]; then
	echo "TESTPATH is not available"
	exit
fi

if [ "x$FWDPORT" = "x" ]; then
	echo "FWDPORT is not available"
	exit
fi

#version="$1"
MEM=$((1024 * 8))
BZIMAGE="arch/x86_64/boot/bzImage"
SYSPATH=/home/boliu/1_work/5_distro
#TESTPATH=/distro
MAIN_DISK="/distro/Fedora-16.img"
SEC_DISK="$TESTPATH/Fedora-sec.img"
ISO="$SYSPATH/Fedora-Live-Workstation-x86_64-23-10.iso"
#ISO="$SYSPATH/Fedora-Workstation-Live-x86_64-27-1.6.iso"
CPU=8

#TEST_D1=/home/boliu/tmpfs/test.img
TEST_D1=$TESTPATH/test.img
TEST_D2=$TESTPATH/disk2.img
TEST_D3=$TESTPATH/disk3.img
#COREOS_IMAGE=/home/boliu/1_mysource/coreos/coreos_production_qemu_image.img
TEST_D4=$TESTPATH/disk4.img
TEST_D5=$TESTPATH/disk5.img
TEST_D6=$TESTPATH/disk6.img

GDB="-gdb tcp:localhost:7499"

#virtio-blk does not support discard (trim isn't supported)
#-append 'root=/dev/sda1 ro netconsole=6665@10.0.2.15/eth0,6666@10.0.2.2/ console=ttyS0' \
#-net user,vlan=0,hostfwd=tcp::2222-:22 \
#-append 'root=/dev/sda1 ro console=ttyS0 kmemleak=off ftrace_dump_on_oops memmap=4G!4G' \

/usr/bin/qemu-system-x86_64 -enable-kvm -nographic \
-kernel ${BZIMAGE}  \
-append 'root=/dev/sda1 ro console=ttyS0 kmemleak=off ftrace_dump_on_oops' \
-m ${MEM} \
-cdrom ${ISO} \
$GDB \
-smp $CPU -net nic,vlan=0,model=virtio \
-net user,vlan=0,hostfwd=tcp::${FWDPORT}-:22 \
-device virtio-scsi,id=scsi \
-drive file=${MAIN_DISK},if=none,id=hd1,aio=native,discard=unmap,cache=none -device scsi-hd,drive=hd1 \
-drive file=${SEC_DISK},if=none,id=hd2,aio=native,discard=unmap,cache=none -device scsi-hd,drive=hd2 \
-drive file=${TEST_D1},if=none,id=hd3,aio=native,discard=unmap,cache=none,format=raw -device scsi-hd,drive=hd3 \
-drive file=${TEST_D2},if=none,id=hd4,aio=native,discard=unmap,cache=none,format=raw -device scsi-hd,drive=hd4 \
-drive file=${TEST_D3},if=none,id=hd5,aio=native,discard=unmap,cache=none,format=raw -device scsi-hd,drive=hd5 \
-drive file=${TEST_D4},if=none,id=hd6,aio=native,discard=unmap,cache=none,format=raw -device scsi-hd,drive=hd6 \
-drive file=${TEST_D5},if=none,id=hd7,aio=native,discard=unmap,cache=none,format=raw -device scsi-hd,drive=hd7 \
-drive file=${TEST_D6},if=none,id=hd8,aio=native,discard=unmap,cache=none,format=raw -device scsi-hd,drive=hd8 \
-fsdev local,security_model=passthrough,id=fsdev0,path=`pwd` -device virtio-9p-pci,id=fs0,fsdev=fsdev0,mount_tag=hostshare || exit

#-drive file=/dev/sdb6,discard=on
#-drive file=${COREOS_IMAGE},if=virtio \
#-drive file=${TEST_D5},if=virtio


#========================end here===============================
