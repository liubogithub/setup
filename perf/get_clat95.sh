egrep -nr "95.00th=" $1 | awk -v myvar=$2 -F '95.00th=\\[' '{if ((myvar == 1) || (NR % myvar) == 1) print $2}' | awk -F '\\]' '{if ($1 < 100) print $1 * 1000; else print $1}'
