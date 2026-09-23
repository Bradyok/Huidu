killall -9 BootLogo
killall -9 cn.huidu.BoxPlayerLoader

pm list package | grep cn.huidu.BoxPlayerLoader
if [ $? == "1" ]; then
    pm install /system/usr/app/BoxPlayerLoader.apk
fi
wm density 160
am start -n cn.huidu.BoxPlayerLoader/cn.huidu.MainActivity