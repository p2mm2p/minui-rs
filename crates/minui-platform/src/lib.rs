//! # MinUI Platform
//!
//! 平台抽象层 —— Platform trait 及其关联类型。
//! 同时提供依赖 Platform 关联常量的泛型路径函数。

pub mod platform;
pub mod paths;

pub use platform::{
    Platform, Framebuffer, GfxRenderer, AudioFrame, LidContext,
    ScalerFn, NA,
};

#[cfg(test)]
pub mod test_platform {
    //! 重新导出测试平台，方便外部 crate 的测试使用
    pub use crate::platform::test_platform::TestPlatform;
}
