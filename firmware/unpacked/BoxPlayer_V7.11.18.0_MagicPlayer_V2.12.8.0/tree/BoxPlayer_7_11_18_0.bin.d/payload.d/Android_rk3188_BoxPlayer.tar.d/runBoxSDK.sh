# !/bin/sh

echo "start BoxSDK process..."
killall -9 BoxSDK
killall -9 cn.huidu.BoxSDKLoader
am start -n cn.huidu.BoxSDKLoader/cn.huidu.BoxSDKLoaderActivity