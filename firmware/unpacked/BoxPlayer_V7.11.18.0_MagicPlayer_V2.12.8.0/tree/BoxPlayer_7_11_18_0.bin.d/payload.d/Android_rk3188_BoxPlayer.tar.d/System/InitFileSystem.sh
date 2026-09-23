

set -x
chmod 777 /etc/wifi.sh
chmod 777 /usr/share/udhcpc/default.script
chmod 777 /system
chmod 777 /system/etc
chmod 777 /system/var
chmod 666 /system/etc/entropy.bin
chmod 666 /system/etc/hostapd.conf

if [ ! -e /var/run ]
then
    mkdir -p /var/run/hostapd
    mkdir -p /var/run/wpa_supplicant
    chmod 777 /var/run
    chmod 777 /var/run/*
fi

if [ ! -e /etc/wpa_supplicant ]
then
    mkdir -p /etc/wpa_supplicant
    chmod 777 /etc/wpa_supplicant
fi


if [ ! -L /system/root/usb_dev ]
then
    rm -rf /system/root/usb_dev
    ln -s /mnt/usb_storage /system/root/usb_dev
fi


mount -o rw,remount / /
if [ ! -L /root ]
then
   rm -rf /root
   ln -s /system/root /root
fi


if [ ! -L /boot ]
then
    rm -rf /boot
    ln -s /system/boot /boot
fi

if [ ! -L /system/bin/watchdogd ]
then
    ln -s /init /system/bin/watchdogd
fi

if [ ! -L /system/bin/unzip ]
then
    rm -rf /system/bin/unzip
    ln -s /system/bin/busybox-armv7l /system/bin/unzip
fi


ln -s /system/boot /system/root/Box/data
ln -s /system/root/Box/lib/libcurl.so.5.4.0 /system/root/Box/lib/libcurl.so
ln -s /system/root/Box/lib/libcurl.so.5.4.0 /system/root/Box/lib/libcurl.so.5
