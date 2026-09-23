export LD_LIBRARY_PATH=/root/Box/BoxPlayer:/usr/local/lib/:/lib:/usr/lib:/root/Box/lib:$LD_LIBRARY_PATH
export QT_QWS_FONTDIR=/root/Box/lib/fonts
export QT_PLUGIN_PATH=/root/Box/lib/plugins/
export PATH=/bin:/sbin:/usr/bin:/usr/sbin:/root/Box/bin
killall -9 BoxSDK 
cd /root/Box/BoxPlayer/
export LD_PRELOAD=/root/Box/BoxPlayer/libjemalloc.so.2
/root/Box/BoxPlayer/BoxSDK kDebug  -platform offscreen
