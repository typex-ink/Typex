# 10 · 版本策略

> Typex 产品设计书 · 第十章
> 本章定义 Typex 的版本号、开发版后缀、发版 tag 与版本字段维护规则。

## 1. 基本规则

Typex 使用 SemVer：`MAJOR.MINOR.PATCH`。

- `MAJOR`：不兼容的用户配置、数据、IPC 或自动更新契约变化。
- `MINOR`：向后兼容的新功能、平台能力扩展或较大的体验改进。
- `PATCH`：向后兼容的 bug 修复、性能优化、文案修正和小范围体验调整。

正式版只使用纯 SemVer，例如 `0.1.2`。开发版在下一个正式版号后追加 `-dev`，例如 `0.1.2-dev`。

## 2. 开发版号

开发版版本号始终指向「下一次计划发布的正式版本」。

如果上一个正式版是 `0.1.1`：

- 下一次只做 patch 发布时，开发版写作 `0.1.2-dev`。
- 下一次做 minor 发布时，开发版写作 `0.2.0-dev`。
- 如果发布范围改变，可以在开发期调整目标版本，例如从 `0.1.2-dev` 改为 `0.2.0-dev`。

开发版只用于主干开发、测试构建和内部分发。开发版不得作为 GitHub Release 的正式发布版本，也不得标记为 `latest` 更新源。

## 3. 正式版号

发正式版时，从当前开发版去掉 `-dev` 后缀。

示例：

| 阶段 | 版本号 |
|---|---|
| 上一个正式版 | `0.1.1` |
| 主干开发版 | `0.1.2-dev` |
| 发版提交 | `0.1.2` |
| 发版 tag | `v0.1.2` |
| 发版后的主干开发版 | `0.1.3-dev` 或 `0.2.0-dev` |

正式版 tag 必须是 `vMAJOR.MINOR.PATCH`，并且不能包含 `-dev` 或其他 prerelease 后缀。

## 4. 版本字段

以下位置必须保持一致：

- `package.json` 的 `version`
- `src-tauri/Cargo.toml` 的 `package.version`
- `src-tauri/tauri.conf.json` 的 `version`
- 应用内「设置 → 关于」显示的版本

关于页应从 Tauri 应用版本读取，不手写常量。若引入其他显示版本号的位置，也必须复用同一来源。

## 5. 发版流程

1. 确定下一个正式版号，并把三处版本字段从 `X.Y.Z-dev` 改为 `X.Y.Z`。
2. 执行发布前检查：`cargo fmt`、`cargo clippy`、`cargo test`、`pnpm build`、`pnpm test`，并按 [09 发布人工回归清单](09-release-checklist.md) 走查关键路径。
3. 确认 tag、三处版本字段和关于页显示一致。
4. 创建并推送正式版 tag：`vX.Y.Z`。
5. GitHub Actions 的 Release workflow 构建草稿 release；平台 build job 产出平台资产，publish job 聚合上传。当前启用 macOS universal DMG，后续 Windows/Linux 适配时接入同一聚合发布流程；更新器产物等 CP-5.4 密钥与公钥就位后启用。
6. 发版完成后，把主干版本号推进到下一目标版本的 `-dev`。

## 6. CI 约束

Release workflow 只接受正式版 tag。CI 必须拒绝：

- tag 不是 `vMAJOR.MINOR.PATCH`
- tag 版本与三处版本字段不一致
- 任一版本字段带 `-dev`

Nightly 或内部测试构建可以使用 `-dev`，但不应复用正式 release tag，也不应覆盖正式更新源。
