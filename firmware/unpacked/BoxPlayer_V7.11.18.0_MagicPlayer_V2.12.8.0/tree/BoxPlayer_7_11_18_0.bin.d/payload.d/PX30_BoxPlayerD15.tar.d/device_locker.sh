#!/bin/sh

version1="`cat $1`"
version2="6.4.9.15"

num=1
result=""
while [ $num -le 4 ];
do
    val1=`echo "$version1" | awk -F '.' '{print $'$num'}'`
    val2=`echo "$version2" | awk -F '.' '{print $'$num'}'`
    
    if [ $val1 -gt $val2 ]
    then
        result="great"
        break
    elif [ $val1 -lt $val2 ]
    then
        result="less"
        break
    fi

    let num++
done

if [ "$num" = "5" ]
then
    result="equal"
fi

if [ "$result" = "less" ] || [ "$result" = "equal" ]
then
    set -x
    rm /root/Box/config/device_locker
    set +x
else
    echo "not remove /root/Box/config/device_locker"
fi