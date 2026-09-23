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
		channel=`cat /etc/hostapd.conf  | grep "channel" | awk -F "=" '{print $2}'`
		iwpriv wlan1 channel_plan $channel

		ps|grep hostapd |grep -v grep
		
		if [ $? -eq 0 ]
		   then 
			ps | grep hostapd | grep -v grep | awk '{print $1}' | sed -e "s/^/kill -9 /g" | sh -
		fi
		
		hostapd /etc/hostapd.conf -B 
		
}
SelectChannel()
{
	
		ifconfig wlan1 up

		if [ $? -ne 0 ]
		then 
			echo "err"
			exit
		fi

		channel1=`iwlist wlan1 scan | grep "(Channel 1)" | wc -l`
		if [ $? -ne 0 ]
		then 
			echo "iwlist wlan1 scan error 1 !"
			exit
		fi

		channel6=`iwlist wlan1 scan | grep "(Channel 6)" | wc -l`
		if [ $? -ne 0 ]
		then 
			echo "iwlist wlan1 scan error 6 !"
			exit
		fi

		channel11=`iwlist wlan1 scan | grep "(Channel 11)" | wc -l`
		if [ $? -ne 0 ]
		then 
			echo "iwlist wlan1 scan error 11 !"
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

		sed  "/ht_capab/d"  -i /etc/hostapd.conf
		sed "/channel=/c channel=$channel_nu"  -i /etc/hostapd.conf
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
		

		
		if [ ! -e /var/lib/dhcp ]
		then 
			mkdir -p /var/lib/dhcp
		fi
		
		if [ ! -e /var/lib/dhcp/dhcpd.leases ]
		then 
			touch /var/lib/dhcp/dhcpd.leases
		fi
		
		ifconfig wlan1 192.168.6.1 up
		sed -i "/ht_capab/d" /etc/hostapd.conf
		sed -i '/^$/d' /etc/hostapd.conf
		
		echo "Start hostapd...."
		if [ `cat /etc/hostapd.conf | grep "channel="`x == "channel=14"x ]
		then
			SelectChannel
		fi
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
		
		if [ ! -e /var/run/wpa_supplicant ]
		then
			mkdir -p /var/run/wpa_supplicant
		fi
		
		busybox ifconfig wlan0 up
		
		ps|grep wpa_supplicant |grep -v grep
		
		if [ $? -ne 0 ]
		then
			wpa_supplicant -B -iwlan0 -Dnl80211 -c/etc/wpa_supplicant/wpa_supplicant.conf
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
  
		
		
