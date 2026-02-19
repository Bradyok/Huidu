export PATH=/root/Box/bin:/root/Box/System:$PATH
export LD_LIBRARY_PATH=/root/Box/BoxPlayer:/usr/local/lib/:/lib:/usr/lib:/root/Box/lib:$LD_LIBRARY_PATH
cd /sys/devices/platform/ff400000.gpu/devfreq/ff400000.gpu
echo userspace > governor
echo 480000000 > userspace/set_freq
cat cur_freq
cd /sys/devices/system/cpu/cpu0/cpufreq/
echo userspace > scaling_governor
cat scaling_available_frequencies
echo 1200000 > scaling_setspeed
cat scaling_cur_freq
clear_fpga
echo 10000 > /proc/sys/vm/vfs_cache_pressure
echo 3 > /proc/sys/vm/drop_caches
sysctl -w vm.overcommit_memory=1
/root/Box/System/BoxDaemon &
sleep 1
killall -1 BoxDaemon
/root/Box/System/BootLogo
killall -3 BoxDaemon
/usr/bin/runBoxUpgrade.sh
#/root/Box/System/InitWayland.sh start
/root/Box/run.sh
/root/Box/bin/start-ssh &
/etc/init.d/S50telnet stop
/root/Box/project/api/cn.huidu.device.api.sh 
/root/Box/project/api/cn.huidu.device.service.sh permanent
