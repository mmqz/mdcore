#!/usr/bin/env python3
"""NSIS 安装器构建器（#228 R4·P0：打包链修复——断链 1/3 的注入与对齐机制）。

三断链 → 本脚本的处置（审计 P2-37，行号以审计基线为准）：
  断链 1  `{{VERSION}}` 占位符无注入机制（示例 `-DVERSION` 与 NSIS
          方言不匹配，字面进 exe 名/注册表）
        → 本脚本从根 Cargo.toml [workspace.package].version（=
          mdcore::VERSION 单源，r24 锁背书）取值渲染模板；渲染后
          残留占位符 / APPVERSION define 不一致 / OutFile 名不含版本
          —— 任一即非零退出（链路可验证）。
  断链 3  `File "..\\.."` 相对路径与 BUILD-VERIFY 示例命令（仓根执行）
          矛盾必 not-found
        → 载荷统一 staging 到 build/nsis/payload/（mdcore.exe =
          mang-shell.exe 重命名 + pdfium.dll），模板 File 路径全部自
          **仓根**起算；本脚本与 makensis 均以仓根为工作目录调用。
  断链 2  WebView2 引导链（URL 定义未用 + ExecWait 幽灵文件）
        → 面移除处置：主线壳 mang-shell（GPU 自绘渲染，DirectX 后端）无 WebView 面
          （windows-check.yml 对旧壳 mdshell-native 的 GTK 排除注释
          同源）；模板不再含 WebView2 段。处置记录见 BUILD-VERIFY §3。

退出码：0 = 全链成功（含产物校验）；非 0 = 任一环节失败（可验证性）。

用法（仓根执行；BUILD-VERIFY §3 同口径）：
    python3 packaging/nsis/build-installer.py \\
        [--exe target/release/mang-shell.exe] \\
        [--pdfium bin/lib/pdfium.dll] \\
        [--makensis <makensis 路径>] [--skip-makensis] [--out-dir build/nsis]

    默认 --exe/--pdfium 为正式出包口径（release + 定位序取件位）；
    CI 内测口径显式传 debug 路径（windows-installer.yml）。
    --skip-makensis：只做 staging+渲染+校验（本地模板验证用，不产安装包）。
"""
from __future__ import annotations

import argparse
import hashlib
import re
import shutil
import subprocess
import sys
from pathlib import Path

# Windows 控制台 Python 默认 cp1252——中文输出即 UnicodeEncodeError
# （CI 首跑实证：windows-installer run 42148212450 死于第一行 print）。
# 强制 UTF-8 I/O（Python 3.7+ reconfigure；不可重配环境降级为替换不炸）。
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

# 仓根 = packaging/nsis/ 的上两级。
ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT / "packaging"))
from version import workspace_version  # noqa: E402（packaging/ 非包内导入）

# NSIS 3.x 方言（模板头注释同源）。makensis 定位序：
#   1. --makensis 显式路径
#   2. PATH 上的 makensis（CI：choco install nsis 后 shim 在位）
#   3. choco 默认安装位（C:\\Program Files (x86)\\NSIS\\makensis.exe）
MAKENSIS_FALLBACKS = [
    Path(r"C:\\Program Files (x86)\\NSIS\\makensis.exe"),
    Path(r"C:\\Program Files\\NSIS\\makensis.exe"),
]

PLACEHOLDER_RE = re.compile(r"\{\{[A-Z_]+\}\}")


def die(msg: str) -> None:
    print(f"[build-installer] FAIL: {msg}", file=sys.stderr)
    sys.exit(1)


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def locate_makensis(explicit: str | None) -> str:
    if explicit:
        p = Path(explicit)
        if not p.is_file():
            die(f"--makensis 指定的文件不存在：{explicit}")
        return str(p)
    found = shutil.which("makensis")
    if found:
        return found
    for cand in MAKENSIS_FALLBACKS:
        if cand.is_file():
            return str(cand)
    die(
        "找不到 makensis（NSIS 3.x）。安装：Windows CI 用 "
        "`choco install nsis`；本地验证见 BUILD-VERIFY §3（Debian nsis 包"
        " + NSISDIR 指路）。"
    )


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument(
        "--exe",
        default="target/release/mang-shell.exe",
        help="mang-shell 可执行文件路径（默认 release 正式口径；CI 内测传 debug）",
    )
    ap.add_argument(
        "--pdfium",
        default="bin/lib/pdfium.dll",
        help="pdfium.dll 路径（默认 = 定位序取件位，windows-test.yml 同源）",
    )
    ap.add_argument("--makensis", default=None, help="makensis 显式路径（缺省自动探测）")
    ap.add_argument(
        "--skip-makensis",
        action="store_true",
        help="只做 staging+渲染+校验，不调用 makensis（本地模板验证）",
    )
    ap.add_argument("--out-dir", default="build/nsis", help="输出目录（默认 build/nsis）")
    args = ap.parse_args()

    out_dir = (ROOT / args.out_dir).resolve()
    payload_dir = out_dir / "payload"
    nsi_out = out_dir / "installer.nsi"
    tpl_path = ROOT / "packaging" / "nsis" / "installer.nsi.tpl"

    # ── 0) 版本单源（断链 1 的取值面）────────────────────────────────
    version = workspace_version()
    print(f"[build-installer] 版本（根 [workspace.package].version = mdcore::VERSION 单源）：{version}")

    # ── 1) 载荷校验 + staging（断链 3 的统一工作目录面）──────────────
    exe_src = (ROOT / args.exe).resolve()
    pdfium_src = (ROOT / args.pdfium).resolve()
    if not exe_src.is_file():
        die(f"--exe 不存在：{exe_src}（先构建 mang-shell，见 BUILD-VERIFY §3 命令）")
    if not pdfium_src.is_file():
        die(
            f"--pdfium 不存在：{pdfium_src}（取件源与 pin 见 docs/INSTALL.md 与 "
            "windows-test.yml 的 Fetch pdfium 步骤）"
        )

    payload_dir.mkdir(parents=True, exist_ok=True)
    # 可执行名 = 自研命名域 mdcore（TRADEMARK_RULES §1；显示名 mang 是
    # A4 定名面，两者解耦——packaging/README 变量表）。
    exe_dst = payload_dir / "mdcore.exe"
    pdfium_dst = payload_dir / "pdfium.dll"
    shutil.copy2(exe_src, exe_dst)
    shutil.copy2(pdfium_src, pdfium_dst)
    print(
        f"[build-installer] staging：{args.exe} -> {exe_dst.relative_to(ROOT)} "
        f"({exe_dst.stat().st_size:,} B)；{args.pdfium} -> {pdfium_dst.relative_to(ROOT)} "
        f"({pdfium_dst.stat().st_size:,} B)"
    )

    # ── 2) 模板渲染（占位符集：VERSION/OUT_FILE/PAYLOAD_EXE/PAYLOAD_DLL）──
    # 路径一律绝对（as_posix——Windows makensis 同收正斜杠）：NSIS 的
    # File/OutFile 按脚本目录解析而非调用 CWD（探针实证见模板头注与
    # BUILD-VERIFY §3），绝对路径在任何解析语义/平台下零歧义。
    out_file = (out_dir / f"mdcore-setup-{version}.exe").resolve().as_posix()
    payload_exe = exe_dst.resolve().as_posix()
    payload_dll = pdfium_dst.resolve().as_posix()
    tpl = tpl_path.read_text(encoding="utf-8")
    rendered = (
        tpl.replace("{{VERSION}}", version)
        .replace("{{OUT_FILE}}", out_file)
        .replace("{{PAYLOAD_EXE}}", payload_exe)
        .replace("{{PAYLOAD_DLL}}", payload_dll)
    )
    nsi_out.parent.mkdir(parents=True, exist_ok=True)
    nsi_out.write_text(rendered, encoding="utf-8", newline="\n")
    print(f"[build-installer] 渲染：{tpl_path.relative_to(ROOT)} -> {nsi_out.relative_to(ROOT)}")

    # ── 3) 渲染校验（链路可验证：不匹配即失败）───────────────────────
    if PLACEHOLDER_RE.search(rendered):
        die(f"渲染产物残留占位符 {{{{...}}}}：{nsi_out}（模板新增占位符须同步本脚本）")
    want_define = f'!define APPVERSION "{version}"'
    if want_define not in rendered:
        die(f"APPVERSION define 与版本单源不一致（期望行：{want_define}）")
    # 产物名必须含版本（exe 名/注册表版本一致性的产物面锚；注册表
    # DisplayVersion 同源 ${APPVERSION}，已由 define 断言闭合）。
    if f"mdcore-setup-{version}.exe" not in rendered:
        die(f"产物名不含版本（期望 mdcore-setup-{version}.exe）")
    print("[build-installer] 渲染校验：占位符零残留 + 版本注入一致 ✓")

    if args.skip_makensis:
        print("[build-installer] --skip-makensis：止于渲染校验（未产安装包）")
        return

    # ── 4) makensis 编译（cwd = 仓根：BUILD-VERIFY 的统一工作目录口径；
    # 脚本内路径已全部绝对化，不依赖该约定也能编译——双层保险）──────
    makensis = locate_makensis(args.makensis)
    proc = subprocess.run(
        [makensis, str(Path(args.out_dir) / "installer.nsi")],
        cwd=ROOT,
    )
    if proc.returncode != 0:
        die(f"makensis 失败（退出码 {proc.returncode}）")

    # ── 5) 产物校验（exe 名/注册表版本一致性的产物面锚）───────────────
    installer = out_dir / f"mdcore-setup-{version}.exe"
    if not installer.is_file() or installer.stat().st_size == 0:
        die(f"安装包产物缺失或为空：{installer}")
    print(
        f"[build-installer] 产物：{installer.relative_to(ROOT)} "
        f"({installer.stat().st_size:,} B)  SHA256 {sha256_of(installer)}"
    )
    print(
        "[build-installer] 完成——注册表 DisplayVersion 与 exe 名同源 "
        f"（APPVERSION={version}，单源注入，无手抄）"
    )


if __name__ == "__main__":
    main()
