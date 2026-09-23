export XDG_RUNTIME_DIR=/tmp/.xdg
export LD_LIBRARY_PATH=/root/Box/BoxPlayer:/usr/local/lib/:/lib:/usr/lib:/root/Box/lib:$LD_LIBRARY_PATH
export QT_QWS_FONTDIR=/root/Box/lib/fonts
export QT_PLUGIN_PATH=/root/Box/lib/plugins/
export PATH=/bin:/sbin:/usr/bin:/usr/sbin:/root/Box/bin
killall -9 BoxPlayer
cd /root/Box/BoxPlayer/
/root/Box/BoxPlayer/BoxPlayer -platform offscreen
