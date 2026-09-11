# clock — 日期时间设置工具

`clock` 是 MinUI 的日期时间设置工具（通用二进制之一，随发布包分发）——用户编辑系统日期/时间并落盘。与原版 C clock 对应，独立进程、不常驻。

## 模块定位与职责

- **交互**：方向键逐字段编辑（年/月/日/时/分/秒）、SELECT 切 12/24 小时制、A 保存（`set_date_time` 系统命令落盘）/ B 取消
- **回卷校验**：闰年/月天数/时分秒边界自动回卷（如 6 月 31 日 → 7 月 1 日），年份钳制 1970..2100
- **偏好持久化**：`show_24hour` 标记文件（存在 = 24 小时制）——与 C 原版文件格式兼容，升级无缝
- **纯逻辑 + 薄装配**：`validate.rs`（字段编辑/回卷）与 `prefs.rs`（偏好读写）纯函数可测；`main.rs` 装配

## 内部结构（子组件）

```
crates/clock/src/
├── main.rs      ← 装配层：启动序列、主循环（脏帧渲染）、保存/退出
├── validate.rs  ← 时间字段编辑纯逻辑：Field 枚举、adjust、validate、display_hour
└── prefs.rs     ← 12/24 小时偏好读写（show_24hour 标记文件）
```

## 详细文档

完整文档（逐字段编辑语义、回卷规则、12/24 切换、光标几何、localtime FFI、设计决策）见 [docs/modules/clock.md](../../docs/modules/clock.md)。
