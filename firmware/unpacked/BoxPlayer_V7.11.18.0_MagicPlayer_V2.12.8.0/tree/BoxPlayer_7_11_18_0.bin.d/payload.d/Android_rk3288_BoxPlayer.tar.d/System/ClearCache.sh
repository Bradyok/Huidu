#!/bin/sh

while [ true ]
do
    busybox-armv7l echo 1 > /proc/sys/vm/drop_caches
    busybox-armv7l free
    busybox-armv7l sleep 5
done
