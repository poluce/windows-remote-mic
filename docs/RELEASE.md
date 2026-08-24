# 发布说明（Windows 打包）

在 **Windows** 机器上构建桌面安装包。

## 前置条件

- Rust（rustup + MSVC Build Tools）
- Node.js 20+
- WebView2（Windows 11 自带，Windows 10 安装时可选自动下载）

## 桌面开发窗口

```powershell
npm install
npm run tauri dev
```

## 构建安装包

```powershell
npm run tauri build
```

产物位置：

```text
src-tauri/target/release/bundle/nsis/
└── <%=appName%>-x64-setup.exe
```

> 安装包为“当前用户安装”，不请求管理员权限，自动带简体中文 + English。

## 校验安装包

```powershell
Get-FileHash -Algorithm SHA256 .\src-tauri\target\release\bundle\nsis\*.exe
```

## 签名说明

- 当前为未签名构建，首次运行 SmartScreen 会提示“未知发布者”，属预期。
- 后续可接入 Authenticode 签名（`bundle.windows.digestAlgorithm` / `certificateThumbprint` 等）。

## GitHub Actions 自动出包

打 tag（如 `v0.1.0`）或手动触发 `Release Windows Build` 工作流后，在 Actions 产物里下载 `.exe`。
