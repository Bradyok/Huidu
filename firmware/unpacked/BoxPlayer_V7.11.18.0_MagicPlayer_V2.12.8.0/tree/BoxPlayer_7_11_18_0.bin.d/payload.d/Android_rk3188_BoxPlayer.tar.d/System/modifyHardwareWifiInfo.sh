#!/bin/sh

Wifi_Type=""
Hardware_File_Path="/etc/hardware.conf"
Hardware_File_Back_Path="$Hardware_File_Path"_back
Old_Line=""
New_Line=""
ID_File=$1
Device_Type=""
Wifi_Device_Type=""
Hardware_Version=""
Log_File=""
Device_File="/system/lib/modules/"
Wifi_Device_Path="/sys/bus/sdio/devices/mmc1:0001:1/"


Echo() {
    if [ -z "$Log_File" ]; then
        echo "$@"
        return 
    fi
    echo "$@" >> "$Log_File"
}


InsmodWifiDevice() {
    if [ -f "$Wifi_Device_Path" ]; then
        return 
    fi

    if [ -f "$Device_File"rk915.ko ]; then
        insmod "$Device_File"rk915.ko   
    fi

    wifiType=$Wifi_Type
    Wifi_Type="rk915"
    CheckDriver
    Wifi_Type=$wifiType
    Echo "device type: $Wifi_Device_Type"
    if [ -n "$Wifi_Device_Type" ]; then
        return
    fi

    if [ -f "$Device_File"rk912.ko ]; then
        insmod "$Device_File"rk912.ko   
    fi
}


GetWifiType() {
    if [ ! -f "$Wifi_Device_Path"vendor ]; then
        Echo "Not Find $Wifi_Device_Path"vendor
    fi
    
    if [ ! -f "$Wifi_Device_Path"device ]; then
        Echo "Not Find $Wifi_Device_Path"device
    fi
    Echo "$(ls -l "$Wifi_Device_Path")"

    vendor=$(cat "$Wifi_Device_Path"vendor)
    device=$(cat "$Wifi_Device_Path"device)
    if [ "$vendor:$device" = "0x0296:0x5347" ]; then
        Wifi_Type="rk912"
    elif [ "$vendor:$device" = "0x0296:0x5348" ]; then
        Wifi_Type="rk915"
    else
        Echo "Not match $vendor:$device"
        Wifi_Type=""
    fi
}


GetHardwareVersion() {
    if [ ! -f $Hardware_File_Path ]; then
        Echo "Not find $Hardware_File_Path"
        return 
    fi

    Hardware_Version=$(grep "version=" "$Hardware_File_Path" | awk -v FS="=" '{print $2}')
}


CheckDriver() {
    if [ -z "$Wifi_Type" ]; then
        Echo "Unknow wifi type"
        return 
    fi

    Wifi_Device_Type=$(lsmod | grep "$Wifi_Type" | awk '{print $1}')
}


CheckHardwareConfigFile() {
    if [ ! -f $Hardware_File_Path ]; then
        Echo "Not find $Hardware_File_Path"
        return 
    fi

    while read -r line
    do
        result=$(echo "$line" | grep "wifi=")
        if [ -z "$result" ]
        then
            continue
        fi

        if [ "$Wifi_Type" = "rk912" ]
        then
            result=$(echo "$line" | grep "wifi=rk912")
            if [ -z "$result" ]
            then
                Old_Line="$line"
                New_Line="wifi=rk912"
            fi
        elif [ "$Wifi_Type" = "rk915" ]
        then
            result=$(echo "$line" | grep "wifi=rk915")
            if [ -z "$result" ]
            then
                Old_Line="$line"
                New_Line="wifi=rk915"
            fi
        fi
        break
    done < $Hardware_File_Path
}


UpdateHardwareConfigFile() {
    if [ ! -f $Hardware_File_Path ]; then
        Echo "Not find $Hardware_File_Path"
        return 
    fi

    if [ -z "$Old_Line" ]; then
        return 
    fi

    sed 's/'"$Old_Line"'/'"$New_Line"'/g' $Hardware_File_Path > $Hardware_File_Back_Path
    mv $Hardware_File_Back_Path $Hardware_File_Path
    sync
}



if [ -z "$ID_File" ]; then
    Echo "ID file not find"
    return ;
fi

Device_Type=$(awk -v FS="-" '{print $1}' "$ID_File")
if [ ! "$Device_Type" = "A3" ]; then
    return 
fi

GetHardwareVersion
if [ ! "$Hardware_Version" = "V3" ]; then
    return 
fi

InsmodWifiDevice
GetWifiType
CheckDriver
if [ -z "$Wifi_Device_Type" ]; then
    Echo "Device Type not match"
    return 
fi

CheckHardwareConfigFile
UpdateHardwareConfigFile
