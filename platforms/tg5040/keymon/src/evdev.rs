//! evdev 按键读取封装（keymon 的系统级按键输入）
//!
//! 本模块对应原版 C `keymon.c` 的设备读取部分——直接读 Linux
//! evdev 设备节点（`/dev/input/event0-3`），不经 SDL。
//!
//! 本模块随 keymon 迁入独立 crate `platform-tg5040-keymon`（原平台 lib 的
//! `src/evdev.rs`）——平台 lib 不再暴露 evdev，keymon 是唯一消费者，
//! 模块作为 keymon 的私有模块（`crate::evdev`）存在。
//!
//! ## 为什么不用 SDL joystick？
//!
//! keymon 是**独立进程**（minui/minarch 崩溃后仍要工作），它只关心
//! 系统级按键（MENU/音量/静音/耳机），直接用内核接口最轻量。
//! 应用级按键（十字键/A/B/...）由 minui/minarch 进程内的 SDL joystick
//! 处理——两者构成双通道输入架构（详见 tg5040 README「系统设置与 keymon」）。
//!
//! ## 与原版 C 的对比
//!
//! 原版 `keymon.c` 直接 `open(O_RDONLY|O_NONBLOCK|O_CLOEXEC)` +
//! `read` 循环读取 `struct input_event`。Rust 版封装为 `InputDevices`
//! 结构体：打开失败容错（跳过该设备）、`Drop` 自动关闭 fd。
//!

/// 设备节点数量。对应原版 keymon.c `INPUT_COUNT 4`。
const INPUT_COUNT: usize = 4;

/// Linux 内核输入事件（`/dev/input/eventX` 读取的原始结构）
///
/// 与内核 `struct input_event`（`linux/input.h`）布局一致——
/// 数据源是内核（`libc::read` 从设备文件读），这是 FFI 边界
/// （Rust ↔ 内核），**必须 `#[repr(C)]`**。
///
/// 注意：与 `settings::Settings` 形成对比——后者读写双方都是本 crate
/// 的 Rust 代码，无跨语言边界，故无 `#[repr(C)]`。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEvent {
    /// 事件时间戳（秒）。对应内核 `timeval.tv_sec`。
    pub sec: i64,
    /// 事件时间戳（微秒）。对应内核 `timeval.tv_usec`。
    pub usec: i64,
    /// 事件类型（`EV_KEY` = 1 / `EV_SW` = 5 等）
    pub kind: u16,
    /// 按键/开关代码
    pub code: u16,
    /// 事件值（0=释放 1=按下 2=重复）
    pub value: i32,
}

/// 输入设备集合（`/dev/input/event0-3`）
///
/// 对应原版 keymon.c 的 `inputs[INPUT_COUNT]` 数组。
/// 打开失败的单设备被跳过（fd 不加入列表）——不 panic，
/// 与原版 `open` 失败返回 -1 后 `read` 循环跳过该 fd 的行为一致。
pub struct InputDevices {
    /// 成功打开的 fd 列表（部分设备打开失败时少于 4 个）
    fds: Vec<i32>,
}

impl InputDevices {
    /// 打开 `/dev/input/event0-3`
    ///
    /// 对应原版 keymon.c `main` 的设备打开循环（keymon.c:88-91）。
    /// 单个设备不存在/无权限时 SHALL 跳过——不影响其他设备。
    pub fn open() -> InputDevices {
        let mut fds = Vec::new();
        for i in 0..INPUT_COUNT {
            let path = format!("/dev/input/event{i}");
            let fd = open_event_device(&path);
            if fd >= 0 {
                fds.push(fd);
            }
        }
        InputDevices { fds }
    }

    /// 从所有设备读取当前可用的全部事件（非阻塞）
    ///
    /// 对应原版 keymon.c 主循环的 `while(read(input, &ev, sizeof(ev))==sizeof(ev))`
    /// 双循环（keymon.c:119-156）。无事件时返回空列表（立即返回，不阻塞）。
    pub fn poll(&self) -> Vec<InputEvent> {
        let mut events = Vec::new();
        // 复用的读取缓冲区（对应原版 static struct input_event ev）
        // # Safety：InputEvent 是 repr(C) 纯整数结构体，零初始化合法
        let mut ev: InputEvent = unsafe { std::mem::zeroed() };
        for &fd in &self.fds {
            loop {
                let n = unsafe {
                    libc::read(
                        fd,
                        (&mut ev as *mut InputEvent).cast(),
                        size_of::<InputEvent>(),
                    )
                };
                if n as usize == size_of::<InputEvent>() {
                    events.push(ev);
                } else {
                    // n == -1（EAGAIN 无事件）或 n == 0（EOF）或部分读——
                    // 非阻塞设备没有更多事件，退出内层循环
                    break;
                }
            }
        }
        events
    }
}

impl Drop for InputDevices {
    /// 关闭所有设备 fd
    ///
    /// 对应原版 keymon.c 退出前的 `close(inputs[i])` 循环（keymon.c:204-207）。
    /// Rust 用 Drop 自动完成——keymon 进程退出（包括 SIGTERM 路径）时无需手动清理。
    fn drop(&mut self) {
        for &fd in &self.fds {
            unsafe { libc::close(fd) };
        }
    }
}

/// 打开单个 evdev 设备节点（非阻塞 + close-on-exec）
///
/// 对应原版 `open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC)`。
///
/// # Safety
///
/// 本函数包含 unsafe 代码（FFI `open`）。调用者需满足的前置条件：
/// - `path` 为有效的 C 字符串（NUL 结尾）
/// - 失败时返回 -1（不 panic）——调用方检查返回值
fn open_event_device(path: &str) -> i32 {
    // # Safety：path 来自内部构造（/dev/input/event{i}，无 NUL），
    // CString 保证 NUL 结尾；失败返回 -1 由调用方处理
    let c_path = std::ffi::CString::new(path).expect("设备路径含 NUL");
    unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    }
}

// ── 测试 ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── InputEvent 布局（跨平台可测）──

    #[test]
    fn input_event_layout_matches_kernel() {
        // 内核 struct input_event：timeval(8+8) + u16 + u16 + i32 = 24 字节
        assert_eq!(size_of::<InputEvent>(), 24);
    }

    #[test]
    fn input_event_offsets() {
        // 用偏移断言确保字段顺序与内核一致（repr(C) 下按声明顺序）
        let base = std::ptr::addr_of!(INIT.sec) as usize;
        let off_sec = std::ptr::addr_of!(INIT.sec) as usize - base;
        let off_usec = std::ptr::addr_of!(INIT.usec) as usize - base;
        let off_kind = std::ptr::addr_of!(INIT.kind) as usize - base;
        let off_code = std::ptr::addr_of!(INIT.code) as usize - base;
        let off_value = std::ptr::addr_of!(INIT.value) as usize - base;
        assert_eq!(off_sec, 0);
        assert_eq!(off_usec, 8);
        assert_eq!(off_kind, 16);
        assert_eq!(off_code, 18);
        assert_eq!(off_value, 20);
    }

    static INIT: InputEvent = InputEvent {
        sec: 0,
        usec: 0,
        kind: 0,
        code: 0,
        value: 0,
    };

    #[test]
    fn poll_with_no_devices_returns_empty() {
        // 无设备环境（开发机）：所有 open 失败 → fds 空 → poll 返回空，不 panic
        let devices = InputDevices::open();
        assert!(devices.poll().is_empty());
    }

    #[test]
    fn open_failure_is_tolerated() {
        // 打开失败不 panic（fds 为空也是合法的 InputDevices）
        let devices = InputDevices::open();
        assert!(devices.fds.len() <= INPUT_COUNT);
    }
}
