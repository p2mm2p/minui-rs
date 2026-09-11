//! `all` 步骤模块
//!
//! toolchain 聚合入口：按 C 原版流水线顺序调用各步骤。
//!
//! 顺序：`setup → build → system → platform → special → tidy → package`
//! （对齐 C 原版 `make PLATFORM=x` 的 common=build+system+cores，
//! 以及 `make` 的 setup/special/package 顺序——详见
//!
//! 本模块是唯一涉及多步骤编排的文件。
//!

use super::{build, package, platform, setup, special, system, tidy};

/// 执行完整流水线：setup → build → system → platform → special → tidy → package。
///
/// # 参数
///
/// - `platform`：目标平台（如 `tg5040`）
/// - `device`：设备参数（如 `smart`/`brick`，透传给 build 与 platform）
///
/// # 返回值
///
/// - `Ok(())`：全部步骤成功
/// - `Err(String)`：任一步骤失败即中止——错误信息由该步骤提供
pub fn run(platform: &str, device: &str) -> Result<(), String> {
    setup::run()?;
    build::run(platform, device)?;
    system::run(platform)?;
    platform::run(platform, device)?;
    special::run()?;
    tidy::run(platform)?;
    package::run(platform, device)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注：all::run 是顺序调用各步骤的胶水（`?` 传播错误），各步骤的错误
    // 路径已由各自模块的单测覆盖；完整流水线的真实端到端验证由
    // build-release-pipeline 变更的 9.1 手工验收承担（依赖完整环境：
    // 容器镜像/cores 网络克隆/系统 zip/git——不适合作为单测，环境一旦
    // 就绪该测试即失效）。故此处不再做真实执行 all 的测试。
}
