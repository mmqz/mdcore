# 快速上手（D-2 · issue #29）

> 目标：从零到「跑起 web 编辑器 + 全量门禁绿」的最短路径。
> 命令均为实测口径（Linux 开发主环境；Windows 增补见 §1.2）。

## 1. 环境准备

### 1.1 Linux（开发主环境）

```bash
# Rust 工具链（仓内 rust-toolchain.toml 钉 1.99.0——进仓自动装，含 wasm32/clippy/rustfmt）
curl -sSfL https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init -o /tmp/rustup-init
chmod +x /tmp/rustup-init && /tmp/rustup-init -y --default-toolchain none --profile minimal --no-modify-path
export PATH="$HOME/.cargo/bin:$PATH"

# 桌面壳系统依赖（GTK3 + WebKitGTK 4.1——mdshell-native 链接面）
# Debian/Ubuntu：
sudo apt-get install -y libgtk-3-dev libwebkit2gtk-4.1-dev pkg-config
# Fedora：sudo dnf install gtk3-devel webkit2gtk4.1-devel pkgconf-pkg-config
# Arch：sudo pacman -S gtk3 webkit2gtk-4.1 pkgconf

# web 面依赖
# node >= 18 / npm；python3 >= 3.10 + playwright（chromium）
python3 -m pip install playwright && python3 -m playwright install chromium
```

受限/离线环境备选：仓外 optroot 提取树（pkg-config/ld/GI 四面前置叠加）
可替代系统安装——`source optroot/env.sh` 后同下。

### 1.2 Windows（CI 口径——本地开发同此）

```powershell
# Rust（rustup 自动读 rust-toolchain.toml；wasm32 target 必装）
rustup target add wasm32-unknown-unknown
# MSVC 面先入 vcvars（link.exe 在场）：
cmd /c "call `"%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat`" && cargo check --workspace"
# webview2 SDK 经 Cargo 自动拉取（锁内）；桌面壳实机运行验证 = 打包轮（未完成）
```

## 2. 构建

```bash
# 引擎核心（全 workspace 15 crate）
cargo build --workspace

# web 链（wasm → JS glue → dist）
cd web
npm install                  # 或 npm ci
npm run build:wasm           # cargo wasm32 release + wasm-bindgen（CLI 版本必须与 Cargo.lock 的 wasm-bindgen 精确一致——当前 0.2.129）
npm run build                # esbuild → dist/
# PDF 阅读面（可选）：npm run build:wasm:pdf（pkg_pdf/——pdf_view 模型面）

# 桌面壳
cargo build -p mdshell-native
```

## 3. 起本地服务

```bash
cd web && npm run serve      # http://localhost:8123（python http.server --directory dist）
```

## 4. 跑测试

```bash
# 引擎面（全量语义锁）
cargo test --workspace

# web e2e（自起服务，无需手动 serve；playwright chromium）
cd web && python3 tests/web_smoke.py          # 冒烟 15 checks
python3 tests/docx_export.py                   # DOCX 导出面 30 checks
# 全部 19 套件：ls tests/*.py 逐跑（结果落 tests/results/*.json）

# PDF 引擎面（可选，需 libpdfium.so）
cargo test -p mdc-pdf --features engine-pdfium

# 合规门
./compliance/scan.sh
```

外部语料常驻门（M-1）：`cargo test -p mdcore --test external_corpus`
（另 `-p mdc-office-io` / `-p mdcore-wasm` 同名测试；基线
`test-artifacts/external-corpus/BASELINE.json`，下降即红）。

## 5. 常见问题（实测踩坑在档）

| 症状 | 原因与解法 |
|---|---|
| `failed to run custom build command for gdk-sys` / `pkg-config` 找不到 gtk | 系统缺 GTK 开发库或 PKG_CONFIG_PATH 未指到——装 §1.1 包，或 `source optroot/env.sh` |
| 链接期 `rust-lld: unable to find library -lgtk-3 / -lwebkit2gtk-4.1` | pkg-config 找到了（编译过）但链接器搜索路径没带上 `.pc` 的 -L——同上 optroot 前置，或核对 `.pc` 文件 prefix 与实际库路径一致 |
| `rustup could not choose a version of cargo` | 仓外目录跑了 cargo（rust-toolchain.toml 只在仓内生效）——`export RUSTUP_TOOLCHAIN=1.99.0-x86_64-unknown-linux-gnu` 或进仓再跑 |
| 磁盘吃紧（target/ 膨胀数 GB） | `export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`（测试二进制 80MB→15MB/件）；长期不用 `cargo clean` 回收 |
| wasm 运行期 link 失败 | wasm-bindgen CLI 与 Cargo.lock 版本不齐——`cargo install wasm-bindgen-cli --version 0.2.129 --locked`（**前台装**——后台化会被会话收割） |
| `cargo install` 中途死掉 | 同上：后台/nohup 化在此环境不稳定，一律前台 |
| web 测试端口残留 `Address already in use` | 测试套件自带 `allow_reuse_address`；仍冲突则换端口或杀残留进程 |
| Windows `cargo check` 报 link.exe 缺失 | vcvars 未加载——见 §1.2 命令形态（vcvars64.bat && cargo 同 shell） |

## 6. 门禁口径速查

| 门 | 命令 | 基线 |
|---|---|---|
| fmt | `cargo fmt --all -- --check` | 0 diff |
| clippy | `cargo clippy --workspace --all-targets` | 恰 5 条 wasm_bindgen 既有基线（canvas/collab/workbench/lib×2），零新增 |
| 测试 | `cargo test --workspace` | 全绿（数量随轮增长——见各 PR 门禁节） |
| wasm32 | `cargo check -p mdcore-wasm --target wasm32-unknown-unknown` | 过 |
| 合规 | `./compliance/scan.sh` | PASS |
| web | `python3 tests/web_smoke.py` | 15/15 |
