/// minarch 命令行入口
///
/// 参数：`minarch <core_path> <rom_path>`
fn main() {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: minarch <core_path> <rom_path>");
        std::process::exit(1);
    }

    let core_path = &args[1];
    let rom_path = &args[2];

    // 编译时选择平台实现
    #[cfg(feature = "platform-rg35xx")]
    let mut platform = unimplemented!("RG35XX platform");
    #[cfg(not(any(
        feature = "platform-rg35xx",
        feature = "platform-miyoomini",
    )))]
    let mut platform = {
        // 默认：用于桌面测试
        log::warn!("No platform feature selected, using stub");
        struct StubPlatform;
        // StubPlatform would need to implement Platform trait
        // For now this won't compile without a platform feature
        panic!("No platform feature enabled. Use --features platform-<name>");
    };

    let font_data = include_bytes!("../../minui/resources/BPreplayBold-unhinted.otf");
    let mut power = power::PowerManager::new();

    if let Err(e) = minarch::run(&mut platform, core_path, rom_path, font_data, &mut power) {
        eprintln!("minarch error: {}", e);
        std::process::exit(1);
    }
}
