# Changelog

本项目的版本变更记录。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## 发版流程

发版 = 更新版本号 + 打 tag → CI 自动出 Release。

1. 确认 `main` 分支通过 CI（`npm run typecheck`、`cargo test`、`cargo clippy`、`cargo check`）。
2. 更新版本号，三处保持一致：
   - `package.json` 的 `version`
   - `src-tauri/tauri.conf.json` 的 `version`
   - `Cargo.toml` 的 `workspace.package.version`
3. 把本次变更写入本文件，并提交（如 `chore: release v0.1.0`）。
4. 推送后打 tag：`git tag v0.1.0 && git push origin v0.1.0`。
5. GitHub Actions `Release Windows Build` 自动构建安装包、生成 `SHA256SUMS.txt`、创建 GitHub Release 并上传产物。
6. 到 GitHub Releases 页面核对安装包与校验和。

> 手动触发时：进入 Actions → `Release Windows Build` → `Run workflow`，填写 tag 名称。构建产物与自动打 tag 流程一致。

## [Unreleased]

### 新增
- 按键映射运行时闭环：新增 `core-dispatch` 调度器（单击/双击/长按/按住重复 → 查映射 → SendInput），映射保存后热更新；诊断页按键测试时自动暂停调度。
- HOGP 旁路状态改为结构化枚举（`idle` / `pending` / `attached` / `unavailable`），前端不再靠中文消息关键字推断。
- 日志功能补齐：
  - `core-log` 自动轮转（超过 2 MiB 轮转，保留 5 份备份）。
  - 诊断页新增日志面板：查看尾部、清空日志、打开日志目录、切换 DEBUG。
  - 新增 Tauri 命令：`get_log_info` / `read_log_tail` / `clear_log` / `open_log_dir` / `set_debug_logging`。
- 发布流程升级：GitHub Actions 自动创建 Release、上传安装包、生成 SHA256。
- 新增 `docs/项目/真机验收.md` 真机验收记录表。

### 变更
- `AGENTS.md` 移除 clean-room / 外部仓库引用限制。
- 同步维护任务清单与规划文档，删除未实际使用的技术栈描述。

## [0.1.0] - 未发布

### 新增
- 初始版本：Tauri 2 + React 桌面壳。
- BLE 扫描 / GATT / ATVV 端点发现。
- ATVV 协议状态机 + IMA/DVI ADPCM 解码。
- WASAPI 音频端点、重采样、VB-CABLE 路由与诊断。
- 按键映射（13 键、单击/双击/长按）。
- HID Raw Input / 低层钩子 / HOGP 旁路捕获框架。
- 语音桥（BLE → ADPCM → 48kHz 立体声输出）。
- 快捷菜单窗口（`public/quick-menu.html`）。
- GitHub Actions CI 与 NSIS 打包配置。
