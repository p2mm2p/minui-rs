# 工具链容器：在 arm64 Linux 上编译 Rust 二进制与 cores（libretro.so）
#
# 为什么需要它：
#   minui-rs 的真机目标全是 ARM Linux（tg5040 等为 aarch64），而开发机是
#   macOS（Apple Silicon）。宿主机直接交叉编译缺三样东西：ARM Linux
#   linker、SDL2 交叉开发库、cores 补丁所需的 aarch64 交叉 gcc。与其在
#   宿主机逐个补齐（SDL2 的 pkg-config 交叉查找是死结，spike 已证），
#   不如把整个编译环境收敛进一个容器。
#
# 为什么容器本机架构 = 目标架构（aarch64）就够了：
#   arm64 容器在 arm64 开发机上原生运行——容器里的 Rust 与 gcc 直接产出
#   aarch64 产物，"交叉编译"退化为本机编译：SDL2 经 pkg-config 本机查找
#   零配置，linker 就是容器默认 cc。这与 cross 工具（在 arm64 Mac 上有
#   未修复 bug，issue #1716）形成对比——本镜像不依赖 cross。
#
# 构建：podman build -t minui-toolchain:latest toolchain/
# 使用：见 xtask（编译命令统一包 podman run 进本镜像）。

FROM arm64v8/ubuntu:24.04

# ---------------------------------------------------------------------------
# 1. 系统工具
#    cores 编译需要 make（执行 makefile）与 git（克隆上游仓库）；Rust
#    工具链安装需要 curl（rustup 下载）。build-essential 提供本机
#    gcc/g++/make——容器内编译 Rust 无需交叉（本机即 aarch64），而
#    cores 的平台补丁用 CROSS_COMPILE 前缀（见第 3 层）。
# ---------------------------------------------------------------------------
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential \
      make \
      git \
      curl \
      ca-certificates \
      pkg-config \
 && rm -rf /var/lib/apt/lists/*

# ---------------------------------------------------------------------------
# 2. SDL2 开发库
#    Rust 平台 crate（platform-tg5040 等）经 sdl2-sys 用 pkg-config 链接
#    SDL2。容器内本机查找（/usr/lib/aarch64-linux-gnu/pkgconfig），
#    不需要任何 PKG_CONFIG_PATH 交叉配置。
# ---------------------------------------------------------------------------
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      libsdl2-dev \
 && rm -rf /var/lib/apt/lists/*

# ---------------------------------------------------------------------------
# 3. aarch64 交叉工具链
#    cores 的 makefile 经平台补丁定义 CC = $(CROSS_COMPILE)gcc 等。容器
#    环境设 CROSS_COMPILE=/usr/bin/aarch64-linux-gnu-（第 5 层 ENV），
#    使补丁解析到真实交叉编译器——对齐 C 原版 union-tg5040-toolchain 的
#    setup-env.sh arm64 分支（CROSS_COMPILE=/usr/bin/aarch64-linux-gnu-）。
#    libzstd-dev：libchdr（picodrive 等核心的 CD 解压层）编译需要
#    <zstd.h>（libchdr_chd.c 直接 include）；容器缺它 cores 编译失败。
# ---------------------------------------------------------------------------
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      gcc-aarch64-linux-gnu \
      g++-aarch64-linux-gnu \
      libzstd-dev \
 && rm -rf /var/lib/apt/lists/*

# ---------------------------------------------------------------------------
# 4. Rust 工具链（本机架构编译 = 目标产物）
#    rustup minimal profile + aarch64-unknown-linux-gnu target。容器本机
#    即 aarch64，cargo build 无需 --target 即产出 aarch64 产物——xtask
#    保留 --target 只为显式（无歧义）。rustup 默认安装在 /root/.cargo。
# ---------------------------------------------------------------------------
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --profile minimal \
 && /root/.cargo/bin/rustup target add aarch64-unknown-linux-gnu

# ---------------------------------------------------------------------------
# 5. 环境变量
#    PATH：把 cargo/rustc 放进来（rustup 装到 /root/.cargo）。
#    CROSS_COMPILE：cores 补丁的 $(CROSS_COMPILE)gcc 前缀（见第 3 层）。
# ---------------------------------------------------------------------------
ENV PATH="/root/.cargo/bin:${PATH}" \
    CROSS_COMPILE=/usr/bin/aarch64-linux-gnu-
