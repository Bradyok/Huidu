#!/bin/sh

WPA_CLI="wpa_cli -p /data/wpa_supplicant"
Net_Index=0
Busybox="busybox-armv7l "
Process=$0
SelfPid=$$


Init() {
    result=$($Busybox pgrep "rtl_supplicant")
    if [ -z "$result" ]; then
        /etc/wifi.sh clear
        /etc/wifi.sh sta &
        exit
    fi
    
    Net_Index=$($WPA_CLI list_network | awk '{print $1}' | sed -n '3p')
    if [ -n "$Net_Index" ]; then
        return 
    fi

    $WPA_CLI add_network > /dev/null
    Net_Index=$($WPA_CLI list_network | awk '{print $1}' | sed -n '3p')
}


UniqueProcess() {
    result=$($Busybox pgrep -f "${0//./\\.}" | grep -v $SelfPid) 
    if [ -z "$result" ]; then
        return 
    fi
    kill "$result"
}


IsConnect() {
    result=$($WPA_CLI status | grep "wpa_state" | awk -v FS="=" '{print $2}')
    if [ ! "$result" = "COMPLETED" ]; then
        echo "1"
    fi

    echo "0"
}


ClearSta() {
    result=$($Busybox pgrep "udhcpc")
    if [ -z "$result" ]; then
        killall udhcpc
    fi 

    $WPA_CLI disable_network "$Net_Index" > /dev/null
    $WPA_CLI remove_network "$Net_Index" > /dev/null
    $WPA_CLI add_network > /dev/null
    ip addr flush dev wlan0
}


SwitchAp() {
    ClearSta

    ssid=\"$1\"
    passwd=$2
    $WPA_CLI set_network "$Net_Index" ssid "$ssid" > /dev/null
    if [ -z "$passwd" ]; then
        $WPA_CLI set_network "$Net_Index" key_mgmt NONE > /dev/null
    else
        $WPA_CLI set_network "$Net_Index" psk \""$passwd"\" > /dev/null
    fi

    #$WPA_CLI save_config > /dev/null
    $WPA_CLI select_network "$Net_Index" > /dev/null
    $WPA_CLI enable_network "$Net_Index" > /dev/null

    killall udhcpc > /dev/null
    udhcpc -i wlan0 -T 1 -A 0 -b -q &
}

if [ "$1" = "sta" ]; then
    UniqueProcess
    Init
    SwitchAp "$2" "$3"
elif [ "$1" = "clear" ]; then
    ClearSta
fi

