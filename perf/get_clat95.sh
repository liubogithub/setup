THREAD=`echo $1 | awk -F 'thread' '{print $2}' | awk -F '-' '{print $1}'`

egrep -nr "95.00th=" $1 | awk -v myind=$THREAD -F '95.00th=\\[' '{if ((myind == 1) || (NR % myind) == 1) print $2}' | awk -F '\\]' '{if ($1 < 100) print $1 * 1000; else print $1}'
