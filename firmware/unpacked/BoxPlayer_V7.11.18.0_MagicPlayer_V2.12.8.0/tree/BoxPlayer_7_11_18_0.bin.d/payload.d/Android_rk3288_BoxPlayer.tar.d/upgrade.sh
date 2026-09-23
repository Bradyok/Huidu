#! /bin/sh

CALL=busybox-armv7l

set -x
need_reboot="true"
chmod 777 /system/root/Box/*.sh
/system/root/Box/stop_all.sh
killall -1 BoxDaemon

cd "$(dirname "$0")"
echo "0" > /root/upgrade.status

pwd=`echo -n $PWD`

if [ -f $pwd/cn.huidu.BoxPlayerLoader.apk ]
then
	if [ -f /system/usr/app/BoxPlayerLoader.apk ]
	then
		rm /system/usr/app/BoxPlayerLoader.apk
	fi
	
	# boxplayer_md5=`busybox-armv7l md5sum /system/usr/app/cn.huidu.BoxPlayerLoader.apk | awk '{ print $1}'`
	chmod 777 $pwd/cn.huidu.BoxPlayerLoader.apk
	cp $pwd/cn.huidu.BoxPlayerLoader.apk /system/usr/app/cn.huidu.BoxPlayerLoader.apk
	pm uninstall cn.huidu.BoxPlayerLoader
	pm install -r $pwd/cn.huidu.BoxPlayerLoader.apk
	rm $pwd/cn.huidu.BoxPlayerLoader.apk
fi

if [ -f $pwd/com.google.android.webview.apk ]
then
	chmod 777 $pwd/com.google.android.webview.apk
    pm uninstall com.google.android.webview
    pm install $pwd/com.google.android.webview.apk
	
	rm $pwd/com.google.android.webview.apk
fi

if [ -f $pwd/framework-res.apk ]
then
	cp $pwd/framework-res.apk /system/framework/framework-res.apk
    chmod 644 /system/framework/framework-res.apk
	
	rm $pwd/framework-res.apk
fi

rm /root/Box/project/log/*.log
cp -rf BoxPlayer/* /root/Box/BoxPlayer/
cp -rf version/* /root/Box/version/
cp -rf System/* /root/Box/System/
cp -rf SystemConfig/* /root/Box/SystemConfig/
cp -rf image/* /root/Box/image/
cp -rf ./stop_all.sh /root/Box/
cp -rf ./dnsmasq /system/bin/dnsmasq
cp -rf ./libffmpeg.so /system/lib/libffmpeg.so
cp -rf ./librkffplayer.so /system/lib/librkffplayer.so
cp -rf ./libisp_isi_drv_TC358749XBG.so /system/lib/hw/libisp_isi_drv_TC358749XBG.so
cp -rf ./camera.rk30board.so /system/lib/hw/camera.rk30board.so

devType=""
if [ -f /boot/id ] 
then
    devType=`awk -F "-" '{print $1}' /boot/id`
fi

if [ -d ssh ]
then
    chmod +x ssh/*
    cp -rf ssh/ssh /system/bin/
    cp -rf ssh/sshd /system/bin/
    cp -rf ssh/ssh-keygen /system/bin/
    cp -rf ssh/start-ssh /system/bin/
    
    destMd5=$($CALL md5sum /system/lib/libssh.so | $CALL awk '{printf $1}')
    srcMd5=$($CALL md5sum ssh/libssh.so | $CALL awk '{printf $1}')
    if [ ! "$destMd5" = "$srcMd5" ]
    then
        cp -rf ssh/libssh.so /system/lib/
    fi
    
    if [ ! -d /etc/ssh ]
    then
        mkdir /etc/ssh
    fi
    cp -rf ssh/sshd_config /etc/ssh
    
    if [ ! -d /data/ssh ]
    then
        mkdir /data/ssh
    fi
fi

devID=`cat /boot/id`
result=$(echo $devID | grep "${devType}-D")
if [[ "$result" != "" ]]
then
    echo "is development device"
    chmod 777 /data -R

	if [ -f $pwd/cn.huidu.BoxSDKLoader.apk ]
	then
		chmod 777 $pwd/cn.huidu.BoxSDKLoader.apk
		pm uninstall cn.huidu.BoxSDKLoader.apk
		pm install $pwd/cn.huidu.BoxSDKLoader.apk
		
		rm $pwd/cn.huidu.BoxSDKLoader.apk
	fi

    if [ -f /system/lib/libandroid_runtime.so ]
    then
        rm -rf /system/lib/libandroid_runtime.so
    fi
    cp -rf ./runBoxSDK.sh /root/Box/BoxPlayer/
    cp -rf ./libandroid_runtime.so /system/lib/
    chmod 666 /system/lib/libandroid_runtime.so
else
    echo "is normal device"
fi

cp -rf ./libgui.so /system/lib/libgui.so
cp -rf ./libui.so /system/lib/libui.so
cp -rf ./libsurfaceflinger.so /system/lib/
cp -rf ./surfaceflinger /system/bin/surfaceflinger

chmod 666 /system/lib/libgui.so
chmod 666 /system/lib/libui.so
chmod 666 /system/lib/libsurfaceflinger.so
chmod 777 /system/bin/surfaceflinger

#rm ./cn.huidu.BoxSDKLoader.apk
rm ./libandroid_runtime.so
rm ./libgui.so
rm ./libui.so
rm ./libsurfaceflinger.so
rm ./surfaceflinger

if [ "$devType" != "B6" ]
then
    devType=""
fi

cp -rf ./write_read_emmc"$devType" /system/bin/write_read_emmc

if [ -f /boot/id ] 
then
    devType=`awk -F "-" '{print $1}' /boot/id`
    if [ "$devType" != "B6" ]
    then
        if [ -f /etc/hardware.conf ]
        then
            devType="A4"
        else
            devType=""
        fi
    fi
fi

if [ -f fpga/fpga"$devType".img ]
then
    cp fpga/fpga"$devType".img /boot/fpga.img
    write_fpga /root/Box/run.sh /dev/cyclone4-0
    write_fpga /root/Box/run.sh /dev/cyclone4-1
fi

if [ -f pppd"$devType" ]
then
    cp pppd"$devType" /system/bin/pppd
    chmod 777 /system/bin/pppd
fi

fileVersion=`cat ./kernel/kernel_version"$devType"`
unameVersion=`/system/bin/busybox-armv7l uname -a`
if [[ $unameVersion != *$fileVersion* ]]
then
    echo "update kernel..."
    cat ./kernel/kernel"$devType".img > /dev/block/platform/ff0f0000.rksdmmc/by-name/kernel
fi

if [ -d ./ssl ]
then
	cp -rf ./ssl/* /root/Box/config/ssl/
fi

rm -rf *

dos2unix /root/Box/System/Init.sh
dos2unix /root/Box/System/InitFileSystem.sh
dos2unix /root/Box/System/runBootLogo.sh
dos2unix /root/Box/System/runBoxDaemon.sh
dos2unix /root/Box/System/runBoxUpgrade.sh
dos2unix /root/Box/BoxPlayer/runBoxSDK.sh
dos2unix /root/Box/BoxPlayer/runBoxPlayer.sh

chmod 777 /root/Box/System/*.sh
chmod 777 /root/Box/BoxPlayer/runBoxSDK.sh
chmod 777 /root/Box/BoxPlayer/runBoxPlayer.sh
chmod 777 /root/Box/ngrok/ngrok
chmod 777 /root/Box/*.sh
chmod 777 /system/bin/dnsmasq
chmod 777 /system/bin/write_read_emmc
chmod 666 /system/lib/libffmpeg.so
chmod 666 /system/lib/librkffplayer.so
chmod 666 /system/lib/hw/libisp_isi_drv_TC358749XBG.so
chmod 666 /system/lib/hw/camera.rk30board.so

echo "1" > /root/upgrade.status

reboot -f

