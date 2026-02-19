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

devType=""
if [ -f /root/Box/data/id ] 
then
    devType=`awk -F "-" '{print $1}' /root/Box/data/id`
    if [ "$devType" == "D15" ] || [ "$devType" == "D35" ]
    then
        devType="D15"
    fi
fi


if [ -d ssh ]
then
    chmod +x ssh/*
    cp -rf ssh/start-ssh /root/Box/bin
    
    if [ ! -d /etc/ssh ]
    then
        mkdir /etc/ssh
    fi
    cp -rf ssh/sshd_config /etc/ssh
fi


if [ -f ./wifi_$devType.sh ]
then
    cp -rf ./wifi_$devType.sh /etc/wifi.sh
fi


if [ -f ./fpga_$devType.img ]
then
    cp ./fpga_$devType.img /boot/fpga.img
    write_fpga
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
chmod 777 /usr/bin/ntfs-3g
chmod 777 /usr/lib/libntfs-3g.so.88.0.0
ln -s libntfs-3g.so.88.0.0 /usr/lib/libntfs-3g.so.88
ln -s libntfs-3g.so.88.0.0 /usr/lib/libntfs-3g.so
rm -rf BoxPlayer/ntfs-3g 
#end

if [ "$devType" == "D15" ]
then
    cat ./kernel_d15_ec200T.img > /dev/block/by-name/boot
fi

rm ./kernel_d15_ec200T.img

cp ./S21mountall.sh /etc/init.d/S21mountall.sh
chmod 777 /etc/init.d/S21mountall.sh
rm ./S21mountall.sh

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
dos2unix /usr/bin/runBoxUpgrade.sh
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
