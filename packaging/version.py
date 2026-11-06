#!/usr/bin/env python3
"""版本单源读取器（R24-c 机制合同 / #228 NSIS·macOS 注入面）。

唯一手写点 = 根 Cargo.toml [workspace.package].version。
`crates/shell/tests/r24_packaging.rs::version_single_source_via_workspace`
已锁「根手写点 == mdcore::VERSION（运行时唯一出口）」——本脚本取同一
值，NSIS / Info.plist 注入链由此闭环（checklist B2：四处注入无手抄）。

用法：
    python3 packaging/version.py          # stdout 打印版本号（如 0.1.0）
    python3 -c "import sys; sys.path.insert(0,'packaging'); from version import workspace_version; print(workspace_version())"
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

# 仓根 = packaging/ 的上一级（本文件在 packaging/version.py）。
ROOT = Path(__file__).resolve().parent.parent


def workspace_version() -> str:
    """返回根 [workspace.package].version（= mdcore::VERSION，r24 锁背书）。

    失败（节缺失 / version 行缺失 / 格式异常）一律抛 SystemExit 并给
    可定位的错误信息——注入链「不匹配即构建失败」的第一道闸。
    """
    cargo_toml = ROOT / "Cargo.toml"
    try:
        lines = cargo_toml.read_text(encoding="utf-8").splitlines()
    except OSError as e:
        sys.exit(f"读取 {cargo_toml} 失败：{e}")

    in_section = False
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("["):
            in_section = stripped == "[workspace.package]"
            continue
        if not in_section:
            continue
        m = re.match(r'^version\s*=\s*"([^"]+)"\s*$', stripped)
        if m:
            return m.group(1)

    sys.exit(
        "Cargo.toml 缺 [workspace.package].version（版本唯一手写点，"
        "checklist B1 / r24_packaging.rs 一致性锁的注入面）"
    )


if __name__ == "__main__":
    print(workspace_version())
