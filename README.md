# ParquetLens

Windows x64 离线 Parquet 桌面查看工具，基于 Rust、Tauri 2 和 Svelte。当前版本 **0.6.0**，自用版。

## 下载

在 [Releases](https://github.com/zhanglegezhang/ParquetLens/releases/latest) 的 **Assets** 中下载：

- `ParquetLens.exe`：免安装版，11.75 MB。
- `ParquetLens_0.6.0_x64-setup.exe`：当前用户安装包，3.08 MB。
- `SHA256SUMS.txt`：校验下载文件。

需要 Microsoft WebView2。本机 Windows 11 验收通过；Windows 10 1809 和无 WebView2 环境尚未实机验收。程序和安装包未签名，不含自动更新。

## 功能

- 读取 footer 显示行数、列类型、NULL、min/max 等统计，不在打开时扫描整份数据。
- 头部与确定性均匀样例预览，Excel、CSV 和数据字典导出。
- 枚举值全量/快速分析、高基数提前终止、本机缓存和任务取消。
- 保留大整数、Decimal、微秒/纳秒时间精度；支持嵌套值显示。
- 多标签、最近文件、主题与时区设置、单实例和中文/空格路径。
- 不上传文件，不启用遥测；不注册右键菜单，不接管默认打开方式。

升级保留设置和缓存，卸载默认保留个人数据，可明确选择清理。数据位于 `%LOCALAPPDATA%\ParquetLens`。

## 从源码构建

环境：Windows x64、Rust stable/MSVC、Visual Studio C++ Build Tools 和 Windows SDK、Node.js、WebView2。复现版本及完整测量见 [0.6.0 基准报告](docs/benchmark-0.6.0.md)。

```powershell
npm ci
./scripts/verify.ps1
./scripts/build.ps1
```

输出位于 `src-tauri/target/release/ParquetLens.exe` 和 `src-tauri/target/release/bundle/nsis/`。日常开发运行 `npm run tauri dev`。

Python 测试与基准需要单独准备：

```powershell
python -m venv .venv
.venv/Scripts/python.exe -m pip install -r tests/requirements.txt
.venv/Scripts/python.exe bench/generate.py
.venv/Scripts/python.exe bench/generate_mixed.py
```

`bench/data` 的大文件不提交。仓库保留小型合成/上游测试样例及生成脚本；样例来源与许可证见 `tests/fixtures/README.md` 和子目录说明。运行桌面测试前先正常关闭已有 ParquetLens。历史安装升级验收脚本依赖本机历史安装包，并会实际安装/卸载程序，不属于上述日常构建命令。

## 验证与已知限制

0.6.0 通过 58 项 Rust 测试、2 项前端测试、M1–M4 运行回归，以及全新安装、旧版升级、重装和卸载验收。

`opt-level="z"` 使 exe 比 0.5.1 缩小 34.3%，扫描较旧版变慢，但本机扫描和导出仍在目标内。性能并未全部达标：空闲内存中位数约 209 MB，原基准缓存显示 P95 210 ms，新 WebView 配置启动中位数约 1.03 秒；真正冷启动/冷磁盘未测。完整20轮数据摘要、方法和限制见 [基准报告](docs/benchmark-0.6.0.md)。

## 许可

当前 package.json 标记为 UNLICENSED，暂未添加项目开源许可证。第三方依赖与测试样例保留各自许可证。
