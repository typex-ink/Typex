# Typex 文档集

本目录是 Typex 的长期维护文档集，随代码一起演进。代码与文档冲突时，先更新对应文档，再改代码。

## 文档地图

| 文档 | 作用 |
|---|---|
| [01 产品概述](01-product-overview.md) | 定位、竞品、用户、产品原则 |
| [02 功能规格](02-features.md) | 功能行为、状态、验收标准 |
| [03 模型接入层](03-model-providers.md) | STT / LLM Provider 协议、配置 schema、提示词 |
| [04 设计系统](04-design-system.md) | 品牌、图标、配色、字体、动效、组件规范 |
| [05 UX 规格](05-ux-spec.md) | 窗口、流程、HUD、快捷键、错误文案 |
| [06 代码架构](06-code-architecture.md) | 技术选型、模块边界、IPC、平台实现、性能预算 |
| [07 测试规范](07-testing.md) | 测试分层、场景清单、CI 门槛、人工回归口径 |
| [08 决策清单](08-decisions.md) | 待决策项与 ADR 历史记录 |
| [09 发布人工回归清单](09-release-checklist.md) | 发版前人工参考执行的回归清单 |
| [fixtures/](fixtures/) | 提示词与行为评测语料 |

## 事实来源边界

- 当前功能行为以 [02](02-features.md) 为准。
- Provider 协议、配置 schema、提示词以 [03](03-model-providers.md) 为准。
- UI 外观与 design tokens 以 [04](04-design-system.md) 为准，界面结构与交互以 [05](05-ux-spec.md) 为准。
- 模块归属、IPC、平台实现与性能预算以 [06](06-code-architecture.md) 为准。
- 测试要求以 [07](07-testing.md) 为准。
- [08](08-decisions.md) 记录决策原因与历史。ADR 不覆盖当前规格；若旧 ADR 与当前章节冲突，以当前规格为准，并补充新的 ADR 说明变更原因。
- [09](09-release-checklist.md) 是人工回归参考，不是产品规格；发版时按当前功能状态维护并执行。

## 维护规则

- 改动 IPC 契约、配置 schema、状态机行为、Provider wire shape、UI token 或错误码时，必须同步更新对应章节。
- 路线图和实现进度不在本目录维护，使用 GitHub Issues / Projects / Milestones 管理。
- `fixtures/` 中的语料随提示词 bug 增长；提示词行为变更需附本地评测结果。
