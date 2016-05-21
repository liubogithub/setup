egrep -rn "aggrb" $1 | egrep -i -e 'write|read' | awk '{print $1" " $4}' | awk -F 'aggrb=' '{print $1 " " $2}' | awk -F 'KB/s' '{print $1}' | awk '{print $2}'
