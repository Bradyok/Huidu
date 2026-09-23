#! /bin/sh

# 切换到脚本所在目录
cd "$(dirname "$0")"

# 检查可执行文件是否存在
if [ ! -f "./cn.huidu.device.api" ]; then
    echo "Error: Executable file ./cn.huidu.device.api does not exist"
    exit 1
fi

# 添加执行权限
chmod +x ./cn.huidu.device.api

# 在后台运行程序并丢弃所有输出
./cn.huidu.device.api > /dev/null  2>&1 &
