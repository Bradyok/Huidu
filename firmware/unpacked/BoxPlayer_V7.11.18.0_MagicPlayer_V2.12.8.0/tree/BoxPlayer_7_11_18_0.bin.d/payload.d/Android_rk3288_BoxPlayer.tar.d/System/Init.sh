echo 2 > /proc/cpu/alignment
#setprop media.stagefright.use-nuplayer true
setprop sys.hwc.compose_policy 0
echo 1 > /sys/bus/platform/drivers/usb20_otg/force_usb_mode    

/system/root/Box/System/InitFileSystem.sh
/system/root/Box/System/runBoxDaemon.sh
/system/root/Box/System/runBoxUpgrade.sh
/system/root/Box/System/InitIptables.sh

/system/bin/start-ssh &

if [ -f /boot/logo.bmp ]
then
  /system/root/Box/System/runBootLogo.sh
fi

#/system/root/Box/run.sh
