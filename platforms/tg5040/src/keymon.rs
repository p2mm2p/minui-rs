//! # keymon — 按键监控守护进程 (TG5040)
//!
//! 独立于 minui/minarch 运行，直接读取 /dev/input/event1 的原始 evdev 事件。
//! 处理：MENU+VOL± = 亮度调节，单按 VOL± = 音量调节。

use std::fs;
use std::thread;
use std::time::Duration;

use platform_tg5040::libmsettings::Settings;

// TG5040 Smart Pro evdev 扫描码
mod evdev {
    pub const MENU: u16  = 8;
    pub const PLUS: u16  = 128;
    pub const MINUS: u16 = 129;
}

/// 模拟 SDL_GetTicks
fn get_ticks() -> u32 {
    unsafe {
        let mut ts = std::mem::zeroed();
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
        (ts.tv_sec as u32) * 1000 + (ts.tv_nsec as u32) / 1_000_000
    }
}

fn main() {
    let settings = Settings::new();

    // 打开输入设备（非阻塞 + exec 自动关闭）
    let input_fd = unsafe {
        libc::open(
            b"/dev/input/event1\0" as *const _ as *const libc::c_char,
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if input_fd < 0 {
        eprintln!("keymon: cannot open /dev/input/event1");
        std::process::exit(1);
    }

    #[repr(C)]
    struct InputEvent {
        time: libc::timeval,
        ev_type: u16,
        code: u16,
        value: i32,
    }

    let mut menu_pressed = false;
    let mut up_pressed = false;
    let mut up_just_pressed = false;
    let mut up_repeat_at: u32 = 0;

    let mut down_pressed = false;
    let mut down_just_pressed = false;
    let mut down_repeat_at: u32 = 0;

    let mut ignore = false;
    let mut then = get_ticks();

    loop {
        let now = get_ticks();

        // 苏醒后忽略积压输入（防止误触）
        if now.saturating_sub(then) > 1000 {
            ignore = true;
        }

        // 非阻塞读取 evdev 事件（逐个消费直到 EAGAIN）
        let mut ev: InputEvent = unsafe { std::mem::zeroed() };
        loop {
            let n = unsafe {
                libc::read(
                    input_fd,
                    &mut ev as *mut _ as *mut libc::c_void,
                    std::mem::size_of::<InputEvent>(),
                )
            };
            if n != std::mem::size_of::<InputEvent>() as isize {
                break; // 没数据了（EAGAIN）
            }
            if ignore { continue; }
            if ev.ev_type != 0x01 { continue; } // EV_KEY
            if ev.value > 1 { continue; }       // 过滤内核 REPEAT 事件

            let pressed = ev.value == 1;
            match ev.code {
                evdev::MENU => menu_pressed = pressed,
                evdev::PLUS => {
                    up_pressed = pressed;
                    up_just_pressed = pressed;
                    if pressed { up_repeat_at = now + 300; }
                }
                evdev::MINUS => {
                    down_pressed = pressed;
                    down_just_pressed = pressed;
                    if pressed { down_repeat_at = now + 300; }
                }
                _ => {}
            }
        }

        if ignore {
            menu_pressed = false;
            up_pressed = false; up_just_pressed = false;
            down_pressed = false; down_just_pressed = false;
            up_repeat_at = 0; down_repeat_at = 0;
        }

        // 处理 PLUS（含长按重复）
        if up_just_pressed || (up_pressed && now >= up_repeat_at) {
            if menu_pressed {
                let val = settings.brightness();
                if val < 10 { settings.set_brightness(val + 1); }
            } else {
                let val = settings.volume();
                if val < 20 { settings.set_volume(val + 1); }
                settings.apply_volume();
            }
            if up_just_pressed { up_just_pressed = false; }
            else { up_repeat_at += 100; }
        }

        // 处理 MINUS
        if down_just_pressed || (down_pressed && now >= down_repeat_at) {
            if menu_pressed {
                let val = settings.brightness();
                if val > 0 { settings.set_brightness(val - 1); }
            } else {
                let val = settings.volume();
                if val > 0 { settings.set_volume(val - 1); }
                settings.apply_volume();
            }
            if down_just_pressed { down_just_pressed = false; }
            else { down_repeat_at += 100; }
        }

        then = now;
        ignore = false;
        thread::sleep(Duration::from_micros(16666)); // ~60fps
    }
}
