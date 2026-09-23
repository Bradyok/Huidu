#!/bin/sh

set -x

UpgradeStart()
{
    cd "$(dirname "$0")"
    echo "0" > /root/upgrade.status
    
    killall -1 BoxDaemon
    killall -9 BoxSDK BoxPlayer
    killall pppd
}

UpgradeFinish()
{
    echo "1" > /root/upgrade.status
    sync
    
    write_fpga /root/Box/run.sh
    killall -3 BoxDaemon
    reboot
}

CleanOldFiles()
{
    rm -rf /root/Box/BoxPlayer/*
    rm -rf /root/Box/version/*
    rm /etc/init.d/S66load_wifi_modules
}

CopyFiles()
{
    cp ./BoxPlayer/* /root/Box/BoxPlayer/ -rf
    cp ./version/* /root/Box/version/ -rf
    mkdir /root/Box/BoxPlayer/core/
    mv ./S40network /etc/init.d/
    mv ./System/hwSetting.lua /root/Box/System/
}

ChmodFiles()
{
    chmod 777 /root/Box/BoxPlayer/*
    chmod 666 /root/Box/verison/*
    chmod 777 /etc/init.d/S40network
}

CleanUpgradeFiles()
{
    rm ./* -rf
}


UpgradeStart
CleanOldFiles
CopyFiles
ChmodFiles
CleanUpgradeFiles
UpgradeFinish
