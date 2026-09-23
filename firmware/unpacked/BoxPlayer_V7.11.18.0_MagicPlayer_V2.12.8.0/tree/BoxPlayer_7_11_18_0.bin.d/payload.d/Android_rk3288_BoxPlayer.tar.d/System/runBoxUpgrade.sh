# !/bin/sh

killall -9 BoxUpgrade
echo "start BoxUpgrade process..."
cd /system/root/Box/System/
export LD_LIBRARY_PATH=$PWD:/system/root/Box/BoxPlayer:/system/root/Box/lib:$LD_LIBRARY_PATH
/system/root/Box/System/ProgramLoader kDebug /system/root/Box/System/Process.json BoxUpgrade