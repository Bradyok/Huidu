#!/system/bin/sh

if [ -e fpga.img ]; then
  echo "install fpga.img"
  fpga_img_path="/data/data/cn.huidu.lcd.player/files/HwSet/fpga.img"
  cp fpga.img $fpga_img_path
  chmod 666 $fpga_img_path
fi

install_binary() {
    local file_name=$1
    local display_name=$2
    
    if [ -e "$file_name" ]; then
        echo "install $display_name"
        app_bin_dir="/data/app-bin"
        
        if [ ! -d "$app_bin_dir" ]; then
            mkdir -m 777 $app_bin_dir
        fi
        
        chown system:system $app_bin_dir
        killall $file_name 2>/dev/null
        cp $file_name $app_bin_dir/$file_name
        chmod 777 $app_bin_dir/$file_name
    fi
}

install_script() {
    local file_name=$1
    local display_name=$2
    
    if [ -e "$file_name" ]; then
        echo "install $display_name"
        app_bin_dir="/data/app-bin"
        
        if [ ! -d "$app_bin_dir" ]; then
            mkdir -m 777 $app_bin_dir
        fi
        
        chown system:system $app_bin_dir
        cp $file_name $app_bin_dir/$file_name
        chmod 777 $app_bin_dir/$file_name
    fi
}

install_binary "HSDKProxys" "HSDKProxys(bin)"
install_binary "cn.huidu.device.api" "cn.huidu.device.api(bin)"
install_binary "cn.huidu.device.service" "cn.huidu.device.service(bin)"
install_script "cn.huidu.device.api.sh" "cn.huidu.device.api.sh"
install_script "cn.huidu.device.service.sh" "cn.huidu.device.service.sh"

for path in $(ls *.apk)
do
  echo "install $path"
  pm install -r -d $path
done

sync