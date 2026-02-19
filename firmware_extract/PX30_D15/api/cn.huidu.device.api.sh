#! /bin/sh

cd "$(dirname "$0")"

if [ ! -f "/boot/httpApi" ]
then
    return 
fi

if [ -f "/root/Box/data/id" ]
then
    devModeType=`awk -F "-" '{print $2}' /root/Box/data/id`
    if [ "${devModeType:0:1}" == "D" ]
    then  
        ./cn.huidu.device.api -stdout=false > /dev/null 2>&1 &
        
        for file in ./huidushell_*.sh; do  
            bash "$file"  
        done
    fi
fi


