# tg5040 install 资源

本目录是 tg5040 平台的安装/更新资源（平台子 xtask `copy_install` 装配进发布包）：

| 文件 | 装配目标 | 用途 |
|------|---------|------|
| `boot.sh` | `BOOT/common/tg5040.sh` | 设备 bootloader 入口（special 后成 `.tmp_update/tg5040.sh`）——检测 SD 卡根 `MinUI.zip` 并解压安装/更新系统 |
| `update.sh` | `SYSTEM/tg5040/bin/install.sh` | 系统更新后置脚本（runtrimui.sh 旧固件迁移、tg3040→tg5040 数据迁移） |
| `installing.png` / `updating.png` | `BOOT/common/tg5040/`（smart） | boot.sh 的 `./show ./$ACTION.png` 显示安装/更新画面 |
| `brick/installing.png` / `brick/updating.png` | `BOOT/common/tg5040/`（brick） | brick 设备的安装/更新画面（按 device 选源） |
| `unzip` | `BOOT/common/tg5040/unzip` | boot.sh 的 `./unzip -o` 依赖的解压工具 |

## unzip 来源

`unzip` 是从原版官方发布包（`MinUI-20251127-1-base.zip` 内
`.tmp_update/tg5040/unzip`）提取的 **aarch64 动态链接 ELF**（Info-ZIP
UnZip 6.0，sha256 `63edc8ab2212dcc55698edeac7d49d58260a1f988106aff4915e680d5904af35`）。
该二进制随官方包在真机（TrimUI Smart Pro/Brick）验证过，依赖设备固件的
glibc，兼容性有保证。

C 原版在构建期 clone 并编译 `https://github.com/shauninman/unzip60.git`
（`make -f unix/Makefile.trimuismart unzip`，用 `$(CROSS_COMPILE)gcc`）——
重写版直接沿用其产物而非自行编译：自行编译会引入容器 glibc（Ubuntu
24.04 = 2.39）与设备固件 glibc 版本的兼容风险，且无法真机验证。

许可：Info-ZIP 许可（宽松，允许再分发），与 C 原版同源。
