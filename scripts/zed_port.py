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

# 子段形式 `[workspace.dependencies.<name>]`（如 windows —— r39 发现：
# 此前主段正则在首个 \n\[ 处截断，子段定义全部漏解析，windows 依赖
# 被注释掉。子段条目序列化为 inline table 形并入 ws_deps）。
for sub in re.finditer(r'\[workspace\.dependencies\.([a-zA-Z0-9_-]+)\]\n(.*?)(?=\n\[|\Z)', root_src, re.S):
    name, body = sub.group(1), sub.group(2)
    fields = []
    kv_buf = ''
    for line in body.splitlines():
        if not line.strip() or line.strip().startswith('#'):
            continue
        kv_buf = (kv_buf + ' ' + line.strip()) if kv_buf else line.strip()
        opens = kv_buf.count('{') + kv_buf.count('[')
        closes = kv_buf.count('}') + kv_buf.count(']')
        if opens == closes:
            fields.append(' '.join(kv_buf.split()))
            kv_buf = ''
    if fields:
        ws_deps[name] = '{ ' + ', '.join(fields) + ' }'

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
    # r41 增补：跳过 optional 依赖——feature-gated（如 text 的
    # test-support → dep:util）；不激活该 feature 则永不进编译图，
    # 搬入会无谓扩大闭包（util → path/util_macros/async_zip 雪球）。
    # 若未来激活相应 feature，Cargo 显式报缺（安全方向：可发现非静默）。
    for seg in (normal, targeted):
        for ln in seg.splitlines():
            if 'optional' in ln:
                continue
            for n in names_in(ln):
                if n in internal_ws: out.add(n)
    return out

# ---------- 2. 闭包递归 ----------
def compute_closure(root_crates):
    seen, queue = set(root_crates), list(root_crates)
    while queue:
        c = queue.pop()
        cdir = f'{ZED}/crates/{internal_ws[c]}'
        for d in internal_prod_deps(c, cdir):
            if d not in seen:
                seen.add(d); queue.append(d)
    return seen

# 闭包根：ui（组件库，PR #65 原根）+ gpui_platform（窗口/平台层，
# r39 增补）+ text（编辑器 buffer 基座，r42 增补——#68 阶段 A：
# rope + 锚点替换自研 Vec<String>；text 闭包新增 text/rope/clock
# 三 crate，其余依赖已在闭包）+ zlog/bench_metrics（r42 根固定：
# 前者为 optional prod 依赖（ztracing）/mang-shell 日志面直接引用，
# 后者为 gpui bench-support feature 面——optional 跳过规则会把它们
# 甩出闭包致 [features] dep: 引用悬空解析红，故升为根固定）。
# util 为 text 的 feature-gated optional（test-support），不搬——
# 其 [features] 引用由 _strip_dropped_from_features 清理。
closure = compute_closure(['ui', 'gpui_platform', 'text', 'zlog', 'bench_metrics'])
print(f"[闭包] {len(closure)} crates: {sorted(closure)}")

# ---------- 3. 复制 + 改写 ----------
def rewrite_toml(src, crate_name, dropped):
    """改写单个 crate 的 Cargo.toml。返回改写后文本。"""
    lines = src.splitlines(keepends=True)
    out, section, i, last_plain_section = [], None, 0, None
    in_lints = False
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        # 段头跟踪（[[bench]] 数组段与 [package] 单段都吃——r39 修正：
        # 原单括号正则漏配 [[...]] 头，bench/example 段被照抄导致
        # manifest 解析期「can't find bench」错）
        sec_m = re.match(r'^\[{1,2}([^\]]+)\]{1,2}\s*$', line)
        if sec_m:
            section = sec_m.group(1)
            in_lints = section == 'lints'
            # 依赖子段形式 `[...dependencies.<name>]`（body 为 workspace = true
            # + 可选字段，如 gpui_windows 的 scap——r39 发现）→ 改写为父段头
            # + _emit_dep 展开的 inline 条目。
            sub_m = re.match(r'^(dependencies|target\..*\.(build-)?dependencies)\.([a-zA-Z0-9_-]+)$', section)
            if sub_m:
                parent, dep_name = sub_m.group(1), sub_m.group(3)
                # 吃掉子段 body 到下一段
                i += 1
                body_lines = []
                while i < len(lines) and not lines[i].lstrip().startswith('['):
                    body_lines.append(lines[i]); i += 1
                extra_fields = []
                for bl in body_lines:
                    bls = bl.strip()
                    if not bls or bls.startswith('#') or bls == 'workspace = true':
                        continue
                    extra_fields.append(' '.join(bls.split()))
                # 不发新段头：行并入当前段（TOML 段体延伸至下一头——
                # 子段紧邻父段是 Cargo.toml 惯例，此处即父段体的自然延伸）。
                # 若前一段恰不是父段（罕见），退化为显式父段头（宁冒重复
                # 段头错也不静默错段）。
                if last_plain_section != parent:
                    out.append(f'[{parent}]\n')
                _emit_dep(out, dep_name, parent, dropped, extra=', '.join(extra_fields) or None)
                continue
            last_plain_section = section
            if in_lints and stripped == '':
                pass
            if section in ('bench', 'example', 'test'):
                # [[bench]]/[[example]]/[[test]] 段整体删除——对应目录
                # （benches/examples/tests）在搬运面已排除，无 path 的
                # target 声明会在 manifest 解析期报「can't find ... bench」
                # （r39 发现：gpui_platform/gpui_wgpu 的 bench 段）。
                i += 1
                while i < len(lines) and not lines[i].lstrip().startswith('['):
                    i += 1
                out.append('# [[bench]]/[[example]]/[[test]] 段移植时删除：对应目录不在搬运面\n')
                continue
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

def _parse_dep_def(d):
    """解析单条依赖定义（版本号串或 inline table 文本）为 dict。"""
    d = d.strip()
    if d.startswith('"') or d.startswith('=') or re.match(r'^[\d]', d):
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

def _merge_inline(base_def, extra):
    """合并根表定义与 crate 处 extra 声明（features 拼接去重，其余 extra 覆盖）。"""
    def parse(d):
        return _parse_dep_def(d)
    b, e = parse(base_def), parse(extra) if extra else {}
    merged = dict(b)
    for k, v in e.items():
        if k == 'features' and 'features' in merged:
            fa = re.findall(r'"[^"]*"', merged['features'])
            fb = re.findall(r'"[^"]*"', v)
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
        # 内部 → path。根表若带除 path 外的字段（如 gpui_linux 的
        # default-features = false），与 crate 处 extra 一起合并保真
        # ——r39 增补（此前根表字段被丢弃，闭包内依赖静默吃默认 features）。
        base = {}
        if dep in ws_deps:
            base = _parse_dep_def(ws_deps[dep])
        merged = dict(base)
        if extra:
            merged.update(_parse_dep_def('{' + extra + '}'))
        merged['path'] = f'"../{dep}"'
        out.append(f'{dep} = {{ ' + ', '.join(f'{k} = {v}' for k, v in merged.items()) + ' }\n')
    elif dep in internal_ws:
        # 闭包外内部 crate：dev 面删除；生产面若为 feature-gated
        # optional（r42：text 的 dep:util 仅 test-support 激活）也删除
        # 并留注释；否则报错（真闭包缺口）。
        if is_dev:
            dropped.append(f'{dep} (dev)')
            out.append(f'# [port] dev-dep {dep} 在闭包外，删除\n')
        elif extra and re.search(r'\boptional\s*=\s*true', extra):
            dropped.append(f'{dep} (optional prod, feature-gated)')
            out.append(f'# [port] optional prod-dep {dep} 在闭包外（feature-gated，未激活），删除\n')
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
    # 全量复制（排除 tests/examples/benches/target/docs 等非构建面；
    # derive_refineable = 上游 refineable 源目录内嵌的同名子 crate，
    # 闭包已平级搬运到 crates/derive_refineable，内嵌副本不随行——
    # r38/e2514ae 手工去重后在此固化）
    EXCLUDE = {'tests', 'examples', 'benches', 'target', 'docs', '.cargo_vcs_info.json', 'Cargo.lock', 'derive_refineable'}
    shutil.copytree(src_dir, dst_dir, ignore=shutil.ignore_patterns(*EXCLUDE))
    # 改写 Cargo.toml
    dropped = []
    src_toml = open(f'{src_dir}/Cargo.toml').read()
    new_toml = rewrite_toml(src_toml, name, dropped)
    # r42：被删的 optional prod 依赖若被 [features] 引用（dep:X / "X/..."），
    # 同步从 feature 列表剥离——否则 Cargo 解析期对悬空 dep: 引用报错
    # （text 的 test-support → dep:util）。上游升级重放同规则。
    opt_dropped = [d.split(' ')[0] for d in dropped if 'optional prod' in d]
    if opt_dropped:
        new_toml = _strip_dropped_from_features(new_toml, opt_dropped)
    open(f'{dst_dir}/Cargo.toml', 'w').write(new_toml)
    return dropped

def _strip_dropped_from_features(toml_text, names):
    """从 [features] 段的 feature 列表里移除对已删 optional 依赖的引用
    （"dep:X" / "X/..." / "X" 三形式）——否则 Cargo 解析期报
    "feature includes dep:X with X not being an optional dependency"。"""
    lines = toml_text.splitlines(keepends=True)
    out, in_features = [], False
    for ln in lines:
        sec_m = re.match(r'^\[{1,2}([a-zA-Z0-9_.-]+)\]{1,2}\s*$', ln)
        if sec_m:
            in_features = (sec_m.group(1) == 'features')
            out.append(ln)
            continue
        if in_features:
            m = re.match(r'^([\s]*)([a-zA-Z0-9_-]+)\s*=\s*\[(.*)\]\s*$', ln)
            if m and not ln.strip().startswith('#'):
                items = [x.strip() for x in m.group(3).split(',') if x.strip()]
                keep = []
                for it in items:
                    bare = it.strip('"')
                    if any(bare == f'dep:{n}' or bare == n or bare.startswith(f'{n}/') for n in names):
                        continue
                    keep.append(it)
                out.append(f'{m.group(1)}{m.group(2)} = [' + ', '.join(keep) + ']\n')
                continue
        out.append(ln)
    return ''.join(out)

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
