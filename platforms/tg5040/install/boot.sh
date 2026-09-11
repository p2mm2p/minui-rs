#!/bin/sh
# NOTE: becomes .tmp_update/tg5040.sh

PLATFORM="tg5040"
SDCARD_PATH="/mnt/SDCARD"
UPDATE_PATH="$SDCARD_PATH/MinUI.zip"
SYSTEM_PATH="$SDCARD_PATH/.system"

# for Brick
mount -o remount,rw,async "$SDCARD_PATH"
mount -o remount,rw,async "/mnt/UDISK"

echo userspace > /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor
CPU_PATH=/sys/devices/system/cpu/cpu0/cpufreq/scaling_setspeed
CPU_SPEED_PERF=2000000
echo $CPU_SPEED_PERF > $CPU_PATH

# install/update
if [ -f "$UPDATE_PATH" ]; then
	export LD_LIBRARY_PATH=/usr/trimui/lib:$LD_LIBRARY_PATH
	export PATH=/usr/trimui/bin:$PATH

	# leds_off（设备已编译期确定——每设备独立包，直接关闭所有 LED 路径；
	# 设备特有的路径在其他设备上不存在，echo 失败无害。
	# 对应原 C 的 `if [ "$DEVICE" = "brick" ]` 判断，Rust 版移除——详见 README「设备区分机制」）
	echo 0 > /sys/class/led_anim/max_scale
	echo 0 > /sys/class/led_anim/max_scale_lr
	echo 0 > /sys/class/led_anim/max_scale_f1f2

	cd $(dirname "$0")/$PLATFORM
	if [ -d "$SYSTEM_PATH" ]; then
		ACTION=updating
	else
		ACTION=installing
	fi
	# 安装图在包根目录（每设备独立包，package.sh 打包时放入对应设备的图）
	./show ./$ACTION.png
	
	./unzip -o "$UPDATE_PATH" -d "$SDCARD_PATH" # &> /mnt/SDCARD/unzip.txt
	rm -f "$UPDATE_PATH"
	sync
	
	# the updated system finishes the install/update
	if [ -f $SYSTEM_PATH/$PLATFORM/bin/install.sh ]; then
		$SYSTEM_PATH/$PLATFORM/bin/install.sh # &> $SDCARD_PATH/log.txt
	fi
	
	if [ "$ACTION" = "installing" ]; then
		reboot
	fi
fi

LAUNCH_PATH="$SYSTEM_PATH/$PLATFORM/paks/MinUI.pak/launch.sh"
if [ -f "$LAUNCH_PATH" ] ; then
	exec "$LAUNCH_PATH"
fi
