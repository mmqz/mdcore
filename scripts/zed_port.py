#!/usr/bin/env python3
"""Zed crate 物理搬运器 —— 把闭包 crate 从 Zed 仓搬进独立 workspace。

用法: python3 zed_port.py [--clean]
产出: /home/z/my-project/zed-port-lab/  （独立 cargo workspace）
改写规则:
  [package]   version/edition/publish workspace 继承 → 显式值
  [lints]     workspace = true 段 → 删除（第三方代码不受项目 lint）
  内部依赖(闭包内)   → path 引用
  内部依赖(闭包外, 仅 dev) → 删除（测试面后处理）
  外部依赖     → 从 Zed 根 workspace.dependencies 展开显式定义
"""
import re, os, sys, shutil, json

import os as _os
ZED = _os.environ.get('ZED_UPSTREAM', '/home/z/my-project/zed-upstream')
# 输出目标 = 本仓 third_party/zed（独立 workspace，见 PROVENANCE.md）
LAB = _os.path.dirname(_os.path.dirname(_os.path.abspath(__file__)))

# ---------- 1. 解析 Zed 根 workspace ----------
root_src = open(f'{ZED}/Cargo.toml').read()

# 全部内部 crate（根 workspace path 依赖）
internal_ws = dict(re.findall(r'^([a-zA-Z0-9_-]+)\s*=\s*\{[^}]*path\s*=\s*"crates/([^"]+)"', root_src, re.M))
internal_ws['derive_refineable'] = 'refineable/derive_refineable'

# 根 workspace.dependencies 原文条目（外部依赖展开源）——多行定义括号配平聚合
ws_deps_sec = re.search(r'\[workspace\.dependencies\]\n(.*?)(?=\n\[)', root_src, re.S).group(1)
ws_deps = {}  # name -> 原文定义（去 name= 前缀）
buf = ''
for line in ws_deps_sec.splitlines():
    if not line.strip() or line.strip().startswith('#'):
        if not line.strip().startswith('#'):
            continue
        continue
    buf = (buf + '\n' + line) if buf else line
    # 括号配平检查（{ } 与 [ ] 跨行聚合）
    opens = buf.count('{') + buf.count('[')
    closes = buf.count('}') + buf.count(']')
    if opens == closes:
        m = re.match(r'^([a-zA-Z0-9_-]+)\s*=\s*(.+)$', buf, re.S)
        if m:
            ws_deps[m.group(1)] = ' '.join(m.group(2).split())  # 多行压平成单行
        buf = ''

def dep_sections(src):
    normal, targeted = '', ''
    m = re.search(r'\[dependencies\]\n(.*?)(?=\n\[|\Z)', src, re.S)
    if m: normal = m.group(1)
    for tm in re.finditer(r'\[target\.[^\]]+\.dependencies\]\n(.*?)(?=\n\[|\Z)', src, re.S):
        targeted += tm.group(1)
    return normal, targeted

def names_in(text):
    return re.findall(r'^([a-zA-Z0-9_-]+)(?:\.[a-zA-Z_]+)?\s*=', text, re.M)

def internal_prod_deps(cname, cdir):
    src = open(f'{cdir}/Cargo.toml').read()
    normal, targeted = dep_sections(src)
    out = set()
    for n in names_in(normal) + names_in(targeted):
        if n in internal_ws: out.add(n)
    return out

# ---------- 2. 闭包递归 ----------
def compute_closure(root_crate):
    seen, queue = {root_crate}, [root_crate]
    while queue:
        c = queue.pop()
        cdir = f'{ZED}/crates/{internal_ws[c]}'
        for d in internal_prod_deps(c, cdir):
            if d not in seen:
                seen.add(d); queue.append(d)
    return seen

closure = compute_closure('ui')
print(f"[闭包] {len(closure)} crates: {sorted(closure)}")

# ---------- 3. 复制 + 改写 ----------
def rewrite_toml(src, crate_name, dropped):
    """改写单个 crate 的 Cargo.toml。返回改写后文本。"""
    lines = src.splitlines(keepends=True)
    out, section, i = [], None, 0
    in_lints = False
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        # 段头跟踪
        sec_m = re.match(r'^\[([^\]]+)\]\s*$', line)
        if sec_m:
            section = sec_m.group(1)
            in_lints = section == 'lints'
            if in_lints and stripped == '':
                pass
            if section == 'lints':
                # 跳过 [lints] 段整个（直到下一段）
                i += 1
                while i < len(lines) and not lines[i].lstrip().startswith('['):
                    i += 1
                out.append('# [lints] 移植时删除：第三方代码不受项目 lint 门（Zed workspace lints）\n')
                continue
            out.append(line); i += 1; continue

        if in_lints:
            i += 1; continue

        # [package] 段的 workspace 继承
        if section == 'package':
            if re.match(r'^version\s*\.\s*workspace', line):
                out.append('version = "0.1.0"\n'); i += 1; continue
            if re.match(r'^edition\s*\.\s*workspace', line):
                out.append('edition = "2024"\n'); i += 1; continue
            if re.match(r'^publish\s*\.\s*workspace', line):
                out.append('publish = false\n'); i += 1; continue
            if re.match(r'^rust-version\s*\.\s*workspace', line):
                out.append('rust-version = "1.86"\n'); i += 1; continue
            if re.match(r'^(description|authors|homepage|repository|documentation|keywords|categories|readme)\s*\.\s*workspace', line):
                i += 1; continue  # 删除元数据继承
            out.append(line); i += 1; continue

        # 依赖段处理（dependencies / dev-dependencies / target.*.dependencies / build-dependencies）
        is_dep_sec = section and (
            section in ('dependencies', 'dev-dependencies', 'build-dependencies') or
            re.match(r"target\..*\.(build-|dev-)?dependencies$", section)
        )
        if is_dep_sec:
            # 多行聚合：若行内括号不配平，继续吃后续行直到配平
            if '{' in line or '[' in line:
                opens = line.count('{') + line.count('[')
                closes = line.count('}') + line.count(']')
                j = i
                while opens > closes and j + 1 < len(lines):
                    j += 1
                    line += lines[j]
                    opens += lines[j].count('{') + lines[j].count('[')
                    closes += lines[j].count('}') + lines[j].count(']')
                i = j
            # name.workspace = true
            m = re.match(r'^([a-zA-Z0-9_-]+)\s*\.\s*workspace\s*=\s*true\s*$', line.strip())
            if m:
                dep = m.group(1)
                _emit_dep(out, dep, section, dropped, extra=None)
                i += 1; continue
            # name = { workspace = true, ... } 形式
            m = re.match(r'^([a-zA-Z0-9_-]+)\s*=\s*\{([^}]*)\}\s*$', line.strip())
            if m and 'workspace = true' in m.group(2):
                dep = m.group(1)
                rest = re.sub(r'workspace\s*=\s*true,?\s*', '', m.group(2)).strip().rstrip(',').strip()
                extra = rest if rest else None
                _emit_dep(out, dep, section, dropped, extra=extra)
                i += 1; continue
            # 普通行照抄
            out.append(line); i += 1; continue

        out.append(line); i += 1; continue
    return ''.join(out)

def _merge_inline(base_def, extra):
    """合并根表定义与 crate 处 extra 声明（features 拼接去重，其余 extra 覆盖）。"""
    import re as _re
    def parse(d):
        d = d.strip()
        if d.startswith('"') or d.startswith('=') or _re.match(r'^[\d]', d):
            return {'version': d}
        d = d.strip('{}')
        pairs, buf, depth = [], '', 0
        for ch in d:
            if ch in '[{': depth += 1
            if ch in ']}': depth -= 1
            if ch == ',' and depth == 0:
                if buf.strip(): pairs.append(buf.strip())
                buf = ''
            else: buf += ch
        if buf.strip(): pairs.append(buf.strip())
        out = {}
        for p in pairs:
            k, _, v = p.partition('=')
            out[k.strip()] = v.strip()
        return out
    b, e = parse(base_def), parse(extra) if extra else {}
    merged = dict(b)
    for k, v in e.items():
        if k == 'features' and 'features' in merged:
            fa = _re.findall(r'"[^"]*"', merged['features'])
            fb = _re.findall(r'"[^"]*"', v)
            seen = [x.strip('"') for x in fa]
            for x in fb:
                if x.strip('"') not in seen: seen.append(x.strip('"'))
            merged['features'] = '[' + ', '.join(f'"{x}"' for x in seen) + ']'
        else:
            merged[k] = v
    if set(merged) == {'version'}:
        return merged['version']
    return '{ ' + ', '.join(f'{k} = {v}' for k, v in merged.items()) + ' }'

def _emit_dep(out, dep, section, dropped, extra):
    """输出一条依赖改写结果。"""
    is_dev = section == 'dev-dependencies' or (section and section.endswith('.dev-dependencies'))
    if dep in closure:
        # 内部 → path
        body = f'{{ path = "../{dep}"'
        if extra: body += f', {extra}'
        body += ' }'
        out.append(f'{dep} = {body}\n')
    elif dep in internal_ws:
        # 闭包外内部 crate：dev 面删除，生产面报错（不该出现）
        if is_dev:
            dropped.append(f'{dep} (dev)')
            out.append(f'# [port] dev-dep {dep} 在闭包外，删除\n')
        else:
            raise SystemExit(f"FATAL: 生产依赖 {dep} 在闭包外（闭包计算有缺口）")
    else:
        # 外部 → 从根表展开（与 crate 处 extra 合并：optional/features 等）
        if dep in ws_deps:
            out.append(f'{dep} = {_merge_inline(ws_deps[dep], extra)}\n')
        else:
            dropped.append(f'{dep} (外部未在根表)')
            out.append(f'# [port] {dep} 未在 Zed 根 workspace.dependencies 找到\n')

def copy_crate(name):
    src_dir = f'{ZED}/crates/{internal_ws[name]}'
    dst_dir = f'{LAB}/third_party/zed/crates/{name}'
    if os.path.exists(dst_dir): shutil.rmtree(dst_dir)
    # 全量复制（排除 tests/examples/benches/target/docs 等非构建面）
    EXCLUDE = {'tests', 'examples', 'benches', 'target', 'docs', '.cargo_vcs_info.json', 'Cargo.lock'}
    shutil.copytree(src_dir, dst_dir, ignore=shutil.ignore_patterns(*EXCLUDE))
    # 改写 Cargo.toml
    dropped = []
    src_toml = open(f'{src_dir}/Cargo.toml').read()
    new_toml = rewrite_toml(src_toml, name, dropped)
    open(f'{dst_dir}/Cargo.toml', 'w').write(new_toml)
    return dropped

# ---------- 4. 生成 workspace 根 ----------
def gen_workspace(closure):
    members = sorted(f'"crates/{c}"' for c in closure)
    # [patch.crates-io] 相关条目（与 gpui 链相关的照抄——只扫 patch 段原文）
    patch_lines = []
    keep_patches = ['async-task', 'calloop', 'khronos-egl', 'cpal']
    patch_sec_src = re.search(r'\[patch\.crates-io\]\n(.*?)(?=\n\[)', root_src, re.S)
    if patch_sec_src:
        for line in patch_sec_src.group(1).splitlines():
            if any(re.match(rf'^{p}\s*=', line) for p in keep_patches):
                patch_lines.append(line)
    patch_sec = ''
    if patch_lines:
        patch_sec = '\n[patch.crates-io]\n' + '\n'.join(patch_lines) + '\n'

    toml = f'''# Zed 搬运 workspace —— 由 zed_port.py 生成
# 来源: https://github.com/zed-industries/zed @ {os.popen(f"cd {ZED} && git rev-parse HEAD").read().strip()}
# 决策与清单见 third_party/zed/PROVENANCE.md
[workspace]
resolver = "2"
members = [{', '.join(members)}]

[workspace.package]
edition = "2024"
publish = false

[profile.dev]
split-debuginfo = "unpacked"
incremental = true
codegen-units = 16
debug = "limited"

[profile.dev.build-override]
codegen-units = 16
split-debuginfo = "unpacked"
debug = "limited"

[profile.dev.package]
gpui_macros = {{ opt-level = 3 }}
derive_refineable = {{ opt-level = 3 }}
ui_macros = {{ opt-level = 3 }}

[profile.release]
debug = "limited"
lto = "thin"
codegen-units = 1
{patch_sec}'''
    open(f'{ZED_DIR}/Cargo.toml', 'w').write(toml)

# ---------- main ----------
ZED_DIR = f'{LAB}/third_party/zed'
if '--clean' in sys.argv:
    # 安全清理：仅移除搬运产物（crates/ 与 workspace Cargo.toml），
    # PROVENANCE.md / NOTICES.md / 本脚本等仓内文件绝不动。
    if os.path.exists(f'{ZED_DIR}/crates'):
        shutil.rmtree(f'{ZED_DIR}/crates')
    if os.path.exists(f'{ZED_DIR}/Cargo.toml'):
        os.remove(f'{ZED_DIR}/Cargo.toml')
os.makedirs(f'{ZED_DIR}/crates', exist_ok=True)

report = {}
for name in sorted(closure):
    dropped = copy_crate(name)
    report[name] = dropped

gen_workspace(closure)

print("\n[改写报告] 删除的 dev-deps / 未解析项：")
for name, dropped in report.items():
    if dropped:
        print(f"  {name}: {dropped}")
print(f"\n[产出] {ZED_DIR}/ （{len(closure)} crates + workspace 根）")
print("[下一步] cd", ZED_DIR, "&& cargo build -p gpui  # 最重一环先验证")
