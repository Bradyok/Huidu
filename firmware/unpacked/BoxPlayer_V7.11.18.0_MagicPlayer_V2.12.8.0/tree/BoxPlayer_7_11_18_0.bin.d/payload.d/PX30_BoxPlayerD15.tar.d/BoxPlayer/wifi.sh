#!/bin/sh
echo "start Wifi ...."

Stopsta()
{
		ps|grep wpa_supplicant |grep -v grep
		
		if [ $? -eq 0 ]
		then
			ps | grep wpa_supplicant | grep -v grep | awk '{print $1}' | sed -e "s/^/kill -9 /g" | sh -
		fi

}
StartAP()
{
		ps|grep hostapd |grep -v grep
		
		if [ $? -eq 0 ]
		   then 
			ps | grep hostapd | grep -v grep | awk '{print $1}' | sed -e "s/^/kill -9 /g" | sh -
		fi
		
		hostapd /etc/hostapd.conf -B 
		
}

if [ "$1" = "ap" ]
	then
		echo "start wifi ap ...."
		
		if [ ! -e /var/lib/misc ]
	    then
          mkdir -p /var/lib/misc
        fi
		
		if [ ! -e /var/lib/misc/udhcpd.leases ]
        then
          touch /var/lib/misc/udhcpd.leases
        fi  
		
		if [ ! -e /var/state/dhcp ]
		then 
			mkdir -p /var/state/dhcp
		fi
		
		if [ ! -e /var/state/dhcp/dhcpd.leases ]
		then 
			touch /var/state/dhcp/dhcpd.leases 
		fi
		
		ifconfig wlan1 192.168.6.1 up
		
		echo "Start hostapd...."
		
		Stopsta
		StartAP
		
		echo "Start dhcpd..."
		
		ps |grep dhcpd |grep -v grep
		if [ $? -ne 0 ]
			then
			#ps | grep dhcpd | grep -v grep | awk '{print $1}' | sed -e "s/^/kill -9 /g" | sh -
			dhcpd  -cf /etc/dhcpd.conf &
		fi
		
		echo "Wifi AP finshed!"
	
elif [ "$1" = "sta" ]
	then
	
	#	if [ ! $# == 2 ]
	#	  then
	#		echo "Please input : wifi.sh essid passwd"
    #     exit
	#	fi
	#	echo "ctrl_interface=/var/run/wpa_supplicant" >> wpa_supplicant.conf
	#	wpa_passphrase $1 $2 >> wpa_supplicant.conf
		
		if [ ! -e /var/run/wpa_supplicant ]
		then
			mkdir -p /var/run/wpa_supplicant
		fi
		
		busybox ifconfig wlan0 up
		
		ps|grep wpa_supplicant |grep -v grep
		
		if [ $? -ne 0 ]
		then
			wpa_supplicant -B -iwlan0 -Dwext -c/etc/wpa_supplicant/wpa_supplicant.conf
		fi
		
		udhcpc -i wlan0
elif [ "$1" = "clear" ]
	then
	killall wpa_supplicant
	killall udhcpc
	ifconfig wlan0 0.0.0.0
	ifconfig wlan0 down
	killall hostapd
	killall dhcpd
	ifconfig wlan1 0.0.0.0
	ifconfig wlan1 down
	killall wifi.sh sta
	#ifconfig wlan1 up
fi
  
		
		