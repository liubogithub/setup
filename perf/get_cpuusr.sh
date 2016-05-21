THREAD=`echo $1 | awk -F 'thread' '{print $2}' | awk -F '-' '{print $1}'`

grep -nr -i cpu $1 | awk -v myind=$THREAD '{if (myind == 1 || (NR % myind == 1)) print $0}' | awk -F 'usr=' '{print $2}' | awk -F '%' '{print $1}'
