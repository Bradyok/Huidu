#! /bin/sh

CALL=busybox-armv7l

set -x
need_reboot="true"
/root/Box/stop_all.sh
killall -1 BoxDaemon

cd "$(dirname "$0")"
echo "0" > /root/upgrade.status

rm -rf /root/Box/BoxPlayer/*
cp -rf BoxPlayer/* /root/Box/BoxPlayer/

rm -rf /root/Box/version/*
cp -rf version/* /root/Box/version/

rm -rf /root/Box/System/*
cp -rf System/* /root/Box/System/

rm -rf /root/Box/SystemConfig/*
cp -rf SystemConfig/* /root/Box/SystemConfig/

cp -rf image/* /root/Box/image/

chip=""
if [ -f /system/etc/cpuinfo ]
then
    chip=`cat /system/etc/cpuinfo`
    if [ "$chip" = "RK3188-T" ]
    then
        chip="T"
    fi
fi

#A3 hardware version
versionStr=""
a3Version="version=V3"
hardware=`cat /system/etc/hardware.conf`
result=$(echo $hardware | grep "${a3Version}")
if [[ "$result" != "" ]]
then
    versionStr="A3_V3"
fi

cp -rf start_app"$chip".sh /system/bin/start_app.sh
cp -rf dnsmasq /system/bin/dnsmasq
cp -rf ./log.config /root/Box/project/log/
cp ./simsun.ttc /system/fonts/
cp ./stop_all.sh /root/Box/

devType=""
if [ -f /boot/id ] 
then
    devType=`awk -F "-" '{print $1}' /boot/id`
fi

# ssh update
sshPath=
result=$(echo $devType | grep "C[13]5")
if [ -z "$sshPath" ]
then
    if [ -n "$result" ]
    then
        sshPath=ssh_Cx5
    fi
fi

if [ -z "$sshPath" ]
then
    result=$(echo $devType | grep "A3")
    if [ -n "$result" ]
    then
        sshPath=ssh_Cx5
    fi
fi

if [ -z "$sshPath" ]
then
    result=$(echo $devType | grep "A[456]")
    if [ -n "$result" ]
    then
        sshPath=ssh_Ax
    fi
fi

if [ -d $sshPath ]
then
    chmod +x $sshPath/*
    cp -rf $sshPath/ssh /system/bin/
    cp -rf $sshPath/sshd /system/bin/
    cp -rf $sshPath/ssh-keygen /system/bin/
    cp -rf $sshPath/start-ssh /system/bin/
    
    destMd5=$($CALL md5sum /system/lib/libssh.so | $CALL awk '{printf $1}')
    srcMd5=$($CALL md5sum $sshPath/libssh.so | $CALL awk '{printf $1}')
    if [ ! "$destMd5" = "$srcMd5" ]
    then
        cp -rf $sshPath/libssh.so /system/lib/
    fi
    
    if [ ! -d /etc/ssh ]
    then
        mkdir /etc/ssh
    fi
    cp -rf $sshPath/sshd_config /etc/ssh
    
    if [ ! -d /data/ssh ]
    then
        mkdir /data/ssh
    fi
fi

if [ -d ./api ]
then
    if [ ! -d "/data/data/project/api/" ]
    then
        mkdir /data/data/project/api/
    fi
    
    cp -rf ./api/* /data/data/project/api/
    dos2unix /data/data/project/api/*.sh
    chmod +x /data/data/project/api/*
    
    if [ -s "/data/data/project/api/config/serverhost.config" ]
    then
        if [ ! -f "/boot/httpApi" ]
        then
            touch /boot/httpApi
        fi
    fi
fi

devID=`cat /boot/id`
result=$(echo $devID | grep "${devType}-D")
if [[ "$result" != "" ]]
then
    echo "is development device"
    cp ./cn.huidu.BoxSDKLoader.apk /system/usr/app/cn.huidu.BoxSDKLoader.apk
    cp ./cn.huidu.BoxSDKLoader.apk /data/app/cn.huidu.BoxSDKLoader.apk
    chmod 777 /system/usr/app/cn.huidu.BoxSDKLoader.apk
    chmod 777 /data/app/cn.huidu.BoxSDKLoader.apk

    if [ -f /system/lib/libsurfaceflinger.so ]
    then
        rm -rf /system/lib/libsurfaceflinger.so
    fi
    cp -rf ./libsurfaceflinger.so /system/lib/libsurfaceflinger.so
    chmod 666 /system/lib/libsurfaceflinger.so
    
    cp -rf ./runBoxSDK.sh /root/Box/BoxPlayer/
else
    echo "is normal device"
fi

rm ./cn.huidu.BoxSDKLoader.apk
rm ./libsurfaceflinger.so
rm ./runBoxSDK.sh

if [ "$devType" = "C15" ] || [ "$devType" = "C35" ]
then
    devType="C15"
    if [ -f ./fpga/rk3188_cx5_altera.img ]
    then
        cp ./fpga/rk3188_cx5_*.img /boot/
        chmod 777 ./System/write_fpga
        ./System/write_fpga
    fi

    write_fpga /root/Box/run.sh
elif [ "$devType" = "A3" ]
then
    if [ -f ./fpga/rk3188_a3_altera.img ]
    then
        cp ./fpga/rk3188_a3_*.img /boot/
        chmod 777 ./System/write_fpga
        ./System/write_fpga
    fi

    write_fpga /root/Box/run.sh
fi

if [ -f kernel/kernel_"$devType$chip$versionStr".img ]
then
    cat kernel/kernel_"$devType$chip$versionStr".img > /dev/block/platform/emmc/by-name/kernel
fi

if [ ! -f /root/Box/System/hwSetting.lua ]
then
    cp ./hwSetting.lua /root/Box/System/hwSetting.lua
    chmod 777 /root/Box/System/hwSetting.lua
fi


if [ -d ./ssl ]
then
	cp -rf ./ssl/* /root/Box/config/ssl/
fi

cp ./write_read_emmc_"$devType$chip" /system/bin/write_read_emmc
chmod 777 /system/bin/write_read_emmc

rm -rf *

dos2unix /root/Box/System/Init.sh
dos2unix /root/Box/System/InitFileSystem.sh
dos2unix /root/Box/System/runBootLogo.sh
dos2unix /root/Box/System/runBoxDaemon.sh
dos2unix /root/Box/System/runBoxUpgrade.sh
dos2unix /root/Box/BoxPlayer/runBoxSDK.sh
dos2unix /root/Box/BoxPlayer/runBoxPlayer.sh
dos2unix /system/bin/start_app.sh

chmod 777 /root/Box/System/*.sh
chmod 777 /root/Box/BoxPlayer/runBoxSDK.sh
chmod 777 /root/Box/BoxPlayer/runBoxPlayer.sh
chmod 777 /root/Box/ngrok/ngrok
chmod 777 /root/Box/*.sh
chmod 777 /system/bin/start_app.sh
chmod 777 /system/bin/dnsmasq
chmod 666 /root/Box/project/log/log.config
chmod 666 /system/fonts/simsun.ttc

echo "1" > /root/upgrade.status
sync

reboot -f