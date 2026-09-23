echo 2 > /proc/cpu/alignment
#setprop media.stagefright.use-nuplayer true
setprop sys.hwc.compose_policy 0
echo 1 > /sys/bus/platform/drivers/usb20_otg/force_usb_mode

/system/root/Box/System/InitFileSystem.sh
/system/root/Box/System/runBoxDaemon.sh
/system/root/Box/System/runBoxUpgrade.sh
/system/root/Box/System/InitIptables.sh
ifconfig eth0 up

/system/bin/start-ssh &

if [ -f /boot/logo.bmp ]
then
  /system/root/Box/System/runBootLogo.sh
fi

/system/root/Box/System/modifyHardwareWifiInfo.sh /system/root/Box/data/id
result=`cat /system/etc/hardware.conf | grep version`
if [ "$result" == "version=V3" ] && [ ! -f /boot/logo.bmp ]
then
    /system/root/Box/BoxPlayer/runBoxSDK.sh
fi

/data/data/project/api/cn.huidu.device.api.sh 
/data/data/project/api/cn.huidu.device.service.sh permanent
#/system/root/Box/run.sh