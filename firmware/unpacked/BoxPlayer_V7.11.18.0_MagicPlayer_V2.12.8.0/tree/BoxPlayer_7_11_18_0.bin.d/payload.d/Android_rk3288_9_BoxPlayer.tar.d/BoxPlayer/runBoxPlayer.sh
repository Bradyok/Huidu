killall -9 BootLogo
killall -9 cn.huidu.BoxPlayerLoader

while [ true ]
do
    result=`ps | grep com.android.packageinstaller | grep -v grep`
    if [ "$result" != "" ]
    then
        break
    else
        sleep 0.2
    fi
done
wm density 160
am start -n cn.huidu.BoxPlayerLoader/cn.huidu.MainActivity
