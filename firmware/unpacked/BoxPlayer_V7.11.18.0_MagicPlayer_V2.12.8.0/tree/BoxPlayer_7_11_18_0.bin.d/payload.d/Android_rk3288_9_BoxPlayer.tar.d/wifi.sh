#!/bin/sh

echo $0 $1

ResetDriver()
{
    echo "start reset wifi driver..."
    echo 0 > /sys/class/rkwifi/driver
    echo 1 > /sys/class/rkwifi/driver
    echo "reset wifi driver finish."
}


KillProcess()
{
    echo "killall $1"
    busybox-armv7l ps | grep "$1" | grep -v grep
    if [ $? -eq 0 ]
    then
        busybox-armv7l ps | grep "$1" | grep -v grep | awk '{print $1}' | sed -e "s/^/kill -9 /g" | /bin/sh -
    fi
}


StartSta()
{
    echo "start wpa_supplicant"
    KillProcess "wpa_supplicant"
    wpa_supplicant -B -iwlan0 -Dnl80211 -c/system/root/etc/wpa_supplicant/wpa_supplicant.conf
}


StartUdhcpc()
{
    echo "start udhcpc -i wlan0"
    KillProcess "udhcpc -i wlan0"
    sleep 5
    ifconfig wlan0 down
    sleep 1
    ifconfig wlan0 up
    udhcpc -i wlan0
}


StartAP()
{
    echo "start hostapd"
    KillProcess "hostapd"
    hostapd -B /system/root/etc/hostapd.conf
}


StartDhcpd()
{
    echo "start dnsmasq"
    KillProcess "dnsmasq"
    dnsmasq --keep-in-foreground --no-resolv --no-poll \
        --dhcp-authoritative --dhcp-option-force=43,ANDROID_METERED --pid-file \
        --dhcp-range=192.168.6.100,192.168.6.254,2h < /dev/null &
}


SelectChannel()
{
    echo "start select channel"
    ifconfig wlan0 up

    if [ $? -ne 0 ]
    then 
        echo "err"
        exit
    fi

    channel1=`iwlist wlan0 scan | grep "(Channel 1)" | wc -l`
    if [ $? -ne 0 ]
    then 
        echo "iwlist wlan0 scan error 1 !"
        exit
    fi

    channel6=`iwlist wlan0 scan | grep "(Channel 6)" | wc -l`
    if [ $? -ne 0 ]
    then 
        echo "iwlist wlan0 scan error 6 !"
        exit
    fi

    channel11=`iwlist wlan0 scan | grep "(Channel 11)" | wc -l`
    if [ $? -ne 0 ]
    then 
        echo "iwlist wlan0 scan error 11 !"
        exit
    fi

    echo "channel1 = $channel1,channel6 = $channel6, channel11 = $channel11"
    if [ $channel1 -gt $channel6 ]
    then 
        channel=$channel6
        channel_nu=6
    else
        channel=$channel1
        channel_nu=1
    fi

    if [ $channel -gt $channel11 ]
    then 
        channel=$channel11
        channel_nu=11
    fi

    sed "/channel=/c channel=$channel_nu"  -i /system/root/etc/hostapd.conf
}


if [ "$1" = "ap" ]
then
    ifconfig wlan0 192.168.6.1 up

    if [ `cat /system/root/etc/hostapd.conf | grep "#channel2="`x == "#channel2=13"x ]
    then
        SelectChannel
    fi

    StartAP
    StartDhcpd
    
    echo "wifi ap finshed!"

elif [ "$1" = "sta" ]
then
    StartSta
    StartUdhcpc
    
    echo "wifi station finish"

elif [ "$1" = "clear" ]
then
    pid=`busybox-armv7l ps | grep "hostapd" | grep -v grep | awk '{print $1}'` 
    KillProcess wpa_supplicant
    KillProcess hostapd
    KillProcess dnsmasq
    KillProcess udhcpc
    ifconfig wlan0 0.0.0.0
    ifconfig wlan0 down
    if [ "$pid" != "" ]
    then
        ResetDriver
    fi

    echo "end..."
    killall wifi.sh
fi

