# minput — 按键诊断工具

`minput` 是 MinUI 的按键诊断工具（通用二进制之一，随发布包分发）——把每个按键点亮在屏幕上，SELECT+START 退出。用于验证按键映射/摇杆/肩键是否正常。

## 模块定位与职责

- **按键点亮**：按下的键在面板上点亮（`Hole` 素材 → `Button` 素材切换），摇杆移动在面板上显示方向
- **能力面板**：按平台 `HAS_*` 能力常量渲染（有 L3/R3 才显示 L3/R3 位置——smart 无、brick 有）
- **退出组合**：SELECT+START 同时按下退出（防止误触）
- **纯逻辑 + 薄装配**：`layout.rs`（能力→几何映射、7 组排布）纯函数可测；`main.rs` 装配

## 内部结构（子组件）

```
crates/minput/src/
├── main.rs      ← 装配层：启动序列、主循环（脏帧模式）、SELECT+START 检测
└── layout.rs    ← 布局纯逻辑：Capabilities → 几何映射、ButtonLayout、oy 压缩
```

## 详细文档

完整文档（按键能力面板、布局排布、脏帧模式、退出组合、设计决策）见 [docs/modules/minput.md](../../docs/modules/minput.md)。
