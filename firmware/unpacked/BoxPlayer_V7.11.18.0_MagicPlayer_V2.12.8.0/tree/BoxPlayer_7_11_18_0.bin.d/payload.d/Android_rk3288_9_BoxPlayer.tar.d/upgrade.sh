#! /bin/sh

set -x
/system/root/Box/stop_all.sh
killall -1 BoxDaemon

cd "$(dirname "$0")"
echo "0" > /root/upgrade.status

dev=$(cat /proc/cpuinfo | grep Hardware | awk -F":" '{ print $2}')
if [ "$dev" != " Rockchip RK3288 (Android 9.0)" ]
then
    rm ./* -rf
    reboot -f
fi

mount /dev/block/by-name/system / -t ext4
mount -o remount -o rw /

pwd=`echo -n $PWD`
if [ -f $pwd/cn.huidu.BoxPlayerLoader.apk ]
then
    if [ -f /system/usr/app/BoxPlayerLoader.apk ]
    then
        rm /system/usr/app/BoxPlayerLoader.apk
    fi
    
    #boxplayer_md5=`busybox-armv7l md5sum /system/usr/app/cn.huidu.BoxPlayerLoader.apk | awk '{ print $1}'`
    cp $pwd/cn.huidu.BoxPlayerLoader.apk /system/usr/app/cn.huidu.BoxPlayerLoader.apk
    chmod 777 /system/usr/app/cn.huidu.BoxPlayerLoader.apk
    
    pm uninstall cn.huidu.BoxPlayerLoader
    pm install -r /system/usr/app/cn.huidu.BoxPlayerLoader.apk
    rm $pwd/cn.huidu.BoxPlayerLoader.apk
fi

if [ -f $pwd/wifi.sh ]
then
    cp $pwd/wifi.sh /system/root/etc/
    chmod 777 /system/root/etc/wifi.sh
fi

if [ ! -d /system/root/etc/wpa_supplicant ]
then
    mkdir /system/root/etc/wpa_supplicant
fi

if [ ! -f /system/root/etc/wpa_supplicant/wpa_supplicant.conf ]
then
    if [ -f /system/root/etc/wpa_supplicant.conf ]
    then
        cp /system/root/etc/wpa_supplicant.conf /system/root/etc/wpa_supplicant/wpa_supplicant.conf
        chmod 777 /system/root/etc/wpa_supplicant/wpa_supplicant.conf
    fi
fi

if [ -f fpga/fpga.img ]
then
    cp fpga/fpga.img /system/root/etc/fpga.img
    write_fpga /root/Box/run.sh /dev/cyclone4-0
fi

if [ -f ./dropbear ]
then
    srcMd5=$(md5sum ./dropbear | awk '{print $1}')
    destMd5=$(md5sum /system/bin/dropbear | awk '{print $1}')
    if [ ! "$srcMd5" = "$destMd5" ]; then
        rm -rf /system/bin/dropbear
        cp ./dropbear /system/bin/dropbear
        chmod +x /system/bin/dropbear
    fi
fi

if [ -d ./ssl ]
then
	cp -rf ./ssl/* /data/huidu/config/ssl/
fi

cp ./libsurfaceflinger.so /system/lib/
cp ./libgui.so /system/lib/
chmod 666 /system/lib/libsurfaceflinger.so
chmod 666 /system/lib/libgui.so

dst=/root/Box/
cp ./BoxPlayer/* $dst/BoxPlayer/ -rf
cp ./System/* $dst/System/ -rf
cp ./version/* $dst/version/ -rf
cp ./image/* $dst/image/ -rf
cat ./boot.img > /dev/block/by-name/boot

while true
do
    killall -9 vold
    cp ./vold /system/bin/vold
    if [ $? -eq 0 ]
    then
        break;
    fi
done

rm ./* -rf

chmod 777 $dst/BoxPlayer/*.sh
chmod 777 $dst/System/*

echo "1" > /root/upgrade.status

sync

/system/bin/reboot
