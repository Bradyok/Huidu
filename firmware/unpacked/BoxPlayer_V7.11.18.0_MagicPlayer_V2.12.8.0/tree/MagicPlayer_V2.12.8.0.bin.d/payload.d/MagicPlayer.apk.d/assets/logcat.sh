#!/system/bin/sh

mkdir /sdcard/log
logcat -v time >/sdcard/log/logcat-$(date +%Y-%m%d-%H%M-%S).txt
