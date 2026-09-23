set -x
rm -rf /root/Box/fpga*.img
cd "$(dirname "$0")"
rm /root/Box/project/log/wirelog.*
cp log.config /root/Box/project/log/
rm upgrade.sh
dev=$(cat /proc/cpuinfo | grep Hardware | awk -F":" '{ print $2}')
echo "$dev"
if [ "$dev" = " ZTE ZX296702" ]; then
        echo "is ZTE CPU"
        tar xf ZX296702_BoxPlayerC10,C30,D10,D20,D30.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " Freescale i.MX 6DualLite HD Board" ]; then
        echo "is Freescale iMax6 CPU"
        tar xf iMax6_BoxPlayerA30,A30+,A601,A602,A603.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " RK30board" ]; then
        echo "is  RK30board RK3188 CPU"
        tar xf Android_rk3188_BoxPlayer.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " Rockchip RK3288 (Flattened Device Tree)" ]; then
        echo "is RK3288 CPU"
        tar xf Android_rk3288_BoxPlayer.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " Rockchip RK3288 (Android 9.0)" ]; then
        echo "is RK3288 CPU"
        tar xf Android_rk3288_9_BoxPlayer.tar.gz
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
elif [ "$dev" = " PX30-EVB" ]; then
        echo "is PX30 CPU"
        devType=""
        if [ -f /root/Box/data/id ] 
        then
            devType=`awk -F "-" '{print $1}' /root/Box/data/id`
            if [ "$devType" == "C16" ] || [ "$devType" == "C36" ] || [ "$devType" == "D16" ] || [ "$devType" == "D36" ] || [ "$devType" == "C16L" ] || [ "$devType" == "C08L" ]
            then
                devType="C16"
            fi
        fi
        
        if [ "$devType" == "C16" ]
        then
            tar xf PX30_BoxPlayerD15_RC.tar.gz
        elif [ "$devType" == "D18" ]; then
            tar xf PX30_BoxPlayerD18.tar.gz
        else
            tar xf PX30_BoxPlayerD15.tar.gz
        fi
        
        rm *BoxPlayer*.tar.gz
        ./upgrade.sh
else
        echo "Unknow CPU"
fi
rm /root/Box/*BoxPlayer*.tar.gz
write_fpga /root/Box/run.sh
reboot
