#! /system/bin/sh

# for busybox

if [ `getprop | grep "crypto.state" | busybox awk -F "[" '{print $3}' | busybox awk -F "]" '{print $1}'`x == "encrypted"x ] 
then
    reboot -f
else
    /system/bin/chmod -R 0777 /system/root/
    /system/bin/wdtd &
    /system/root/Box/System/Init.sh
    /system/bin/watchdogd
fi




