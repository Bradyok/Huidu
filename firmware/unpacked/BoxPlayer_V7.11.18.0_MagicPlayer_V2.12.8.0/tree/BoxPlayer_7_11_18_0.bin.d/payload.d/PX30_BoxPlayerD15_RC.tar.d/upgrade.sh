#! /bin/sh
killall -1 BoxDaemon
set -x
need_reboot="false"
killall -9 BoxSDK BoxPlayer
mkdir /root/Box/BoxPlayer/core/
cd "$(dirname "$0")"
echo "0" > /root/upgrade.status

dos2unix ./device_locker.sh
chmod 777 ./device_locker.sh
./device_locker.sh /root/Box/version/version
rm ./device_locker.sh

rm -rf /root/Box/fpga*.img

if [ ! -h /usr/share/fonts/fonts ]
then
    ln -s /usr/lib/fonts /usr/share/fonts
fi

alsactl init
alsactl store

rm /etc/asound.conf
ln -s /root/Box/BoxPlayer/asound.conf /etc/asound.conf

hardwareVersion=`awk -F'=' '/^version=/ {print $2}' /etc/hardware.conf`
devVersion=""
devType=""
ttsRes=""

if [ -f /root/Box/data/id ] 
then
    devType=`awk -F "-" '{print $1}' /root/Box/data/id`
    if [ "$devType" == "D16" ] || [ "$devType" == "D36" ]
    then
        if [ "$devType" == "D16" ]
        then
            if [ "$hardwareVersion" == "V2" ]
            then
                devVersion="_$hardwareVersion"
            fi
        fi
        devType="Dx6"
    else
        if [ "$devType" == "C16L" ]
        then
            devType="C16L"
            ttsRes="ok"
            if [ "$hardwareVersion" == "V3" ]
            then
                devVersion="_$hardwareVersion"
            fi
        elif  [ "$devType" == "C08L" ]
        then
            devType="C08L"
            ttsRes="ok"
        else
            devType="Cx6"
        fi
    fi
fi

if [ "$devType" != "C08L" ]
then
    if [ -d ./Lib ]
    then
        chmod -R 777 ./Lib/*
        chown -R root:root ./Lib/*
        cp -arv ./Lib/* /usr/lib/
    fi
fi

if [ "$ttsRes" == "ok" ]
then
    if [ ! -d /root/Box/project/ttsRes ]
    then
        mkdir /root/Box/project/ttsRes
    fi

    if [ -d ./ttsRes ]
    then
        cp -r ./ttsRes/* /root/Box/project/ttsRes/
    fi
fi

isRK915="false"
hardware=`cat /etc/hardware.conf`
result=$(echo $hardware | grep "wifi=rk915")
if [[ "$result" != "" ]]
then
    isRK915="true"
fi

if [ -f ./fpga/fpga_$devType.img ]
then
    cp ./fpga/fpga_$devType.img /boot/fpga.img
    write_fpga
fi

if [ -f ./boot_$devType$devVersion.img ]
then
    cat ./boot_$devType$devVersion.img > /dev/block/by-name/boot
fi
    
if [ "$devType" != "C16L" ] && [ "$devType" != "C08L" ]
then
    mkdir /root/Box/modules
    if [ -f ./rk912.ko ]
    then
        cp -rf ./rk912.ko /root/Box/modules/
    fi
    if [ -f ./close_wifi.ko ]
    then
        cp -rf ./close_wifi.ko /root/Box/modules/
    fi
fi

if [ -d ./api ]
then
    if [ ! -d "/root/Box/project/api/" ]
    then
        mkdir /root/Box/project/api/
    fi
    
    cp -rf ./api/* /root/Box/project/api/
    dos2unix /root/Box/project/api/*.sh
    chmod +x /root/Box/project/api/*
    
    if [ -s "/root/Box/project/api/config/serverhost.config" ]
    then
        if [ ! -f "/boot/httpApi" ]
        then
            touch /boot/httpApi
        fi
    fi
fi


rm ./boot_*.img
rm ./*.ko

rm ./upgrade.sh
rm -rf /root/Box/BoxPlayer/*
mv -f BoxPlayer/update/* /usr/sbin/
rm -rf BoxPlayer/update/

killall -9 pppd
mv pppd /usr/sbin/pppd
chmod 777 /usr/sbin/pppd
#升级ntfs-3g
mv BoxPlayer/ntfs-3g/ntfs-3g /usr/bin/
mv BoxPlayer/ntfs-3g/libntfs-3g.so.88.0.0 /usr/lib/ 
if [ -f ./librockchip_mpp.so.0 ]
then
    mv ./librockchip_mpp.so.0 /usr/lib/
fi

chmod 777 /usr/bin/ntfs-3g
chmod 777 /usr/lib/libntfs-3g.so.88.0.0
chmod 777 /usr/lib/librockchip_mpp.so.0
ln -s libntfs-3g.so.88.0.0 /usr/lib/libntfs-3g.so.88
ln -s libntfs-3g.so.88.0.0 /usr/lib/libntfs-3g.so
rm -rf BoxPlayer/ntfs-3g 
#end

mv BoxPlayer/log.config /root/Box/project/log/
cp -rf ./System/BoxUpgrade /usr/bin/BoxUpgrade
chmod 777 /usr/bin/BoxUpgrade
tar xf ssl.tar -C /root/Box/config/
cp -rf * /root/Box/
rm -rf *
dos2unix /root/Box/run.sh
dos2unix /root/Box/stop_all.sh
dos2unix /root/Box/BoxPlayer/runBoxSDK.sh
dos2unix /root/Box/BoxPlayer/runBoxPlayer.sh
dos2unix /root/Box/System/BoxPlayerInit.sh
dos2unix /etc/wifi.sh
chmod +x /root/Box/run.sh
chmod +x /root/Box/stop_all.sh
chmod +x /root/Box/BoxPlayer/runBoxSDK.sh
chmod +x /root/Box/BoxPlayer/runBoxPlayer.sh
chmod +x /root/Box/System/write_fpga
chmod +x /root/Box/System/BoxPlayerInit.sh
chmod +x /root/Box/ngrok/ngrok
chmod +x /etc/wifi.sh
echo "1" > /root/upgrade.status
sync

reboot

#killall -3 BoxDaemon
#write_fpga /root/Box/run.sh
#/root/Box/run.sh
