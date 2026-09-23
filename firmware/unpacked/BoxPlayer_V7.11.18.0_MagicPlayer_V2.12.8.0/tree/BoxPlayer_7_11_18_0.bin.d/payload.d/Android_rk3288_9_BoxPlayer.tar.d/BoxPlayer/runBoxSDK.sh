# !/bin/sh

killall -9 BoxSDK
echo "start BoxSDK process..."
cd /system/root/Box/BoxPlayer
export LD_LIBRARY_PATH=$PWD:/system/root/Box/BoxPlayer:/system/root/Box/lib:$LD_LIBRARY_PATH
/system/root/Box/System/ProgramLoader kDebug /system/root/Box/System/Process.json BoxSDK
