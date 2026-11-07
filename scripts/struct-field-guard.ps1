#!/usr/bin/env pwsh
# struct-field-guard.ps1 — 结构体加字段后的字面量一致性检查
#
# 为什么需要这个：
#   2026-11-07 单日三次同类事故：
#     ① Anchor 加 block_idx/page/line（#285②）→ 6 个分支编译失败
#     ② ParagraphData 加 resolved_number（#266）→ PR #299 红
#     ③ PageGeometry 加 media_box/crop_box（#304）→ main 红
#
#   共同根因：给 Rust 结构体加字段后，只改了 struct 定义，
#   没检查所有用「结构体字面量」构造它的地方。
#
#   关键事实：# [serde(default)] 只让反序列化兼容；
#   Rust 的结构体字面量初始化仍是全字段必需，少一个就 E0063。
#
# 用法：
#   ./struct-field-guard.ps1 -Repo <path>            报告模式
#   ./struct-field-guard.ps1 -Repo <path> -Strict    CI 模式（有风险即红）

param(
    [Parameter(Mandatory = $true)][string]$Repo,
    [switch]$Strict
)

$ErrorActionPreference = 'Stop'

$cratesDir = Join-Path $Repo 'crates'
if (-not (Test-Path $cratesDir)) { Write-Error "找不到 crates: $cratesDir"; exit 2 }

$rsFiles = Get-ChildItem -Path $cratesDir -Recurse -Filter '*.rs' -ErrorAction SilentlyContinue
if (-not $rsFiles) { Write-Error "无 .rs 文件"; exit 2 }

# ---- 第 1 步：找带 serde(default) 字段的结构体
$risky = @{}
foreach ($f in $rsFiles) {
    $lines = Get-Content $f.FullName -ErrorAction SilentlyContinue
    if (-not $lines) { continue }
    $curStruct = $null; $defaultCount = 0
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $ln = $lines[$i]
        if ($ln -match '^\s*(?:pub\s+)?struct\s+(\w+)') {
            if ($curStruct -and $defaultCount -gt 0) { $risky[$curStruct] = @{ File = $f.FullName; N = $defaultCount } }
            $curStruct = $Matches[1]; $defaultCount = 0
        } elseif ($ln -match '^\s*#\[serde\(default\)\]') { $defaultCount++
        } elseif ($ln -match '^\}') {
            if ($curStruct -and $defaultCount -gt 0) { $risky[$curStruct] = @{ File = $f.FullName; N = $defaultCount } }
            $curStruct = $null; $defaultCount = 0
        }
    }
}

if ($risky.Count -eq 0) { Write-Host "[struct-guard] OK: 没有加过字段的结构体"; exit 0 }

# ---- 第 2 步：统计字面量构造点
$allContent = @{}
foreach ($f in $rsFiles) { $allContent[$f.FullName] = Get-Content $f.FullName -Raw -ErrorAction SilentlyContinue }

$report = @()
foreach ($name in $risky.Keys) {
    $info = $risky[$name]
    $sites = @()
    foreach ($path in $allContent.Keys) {
        $content = $allContent[$path]
        if (-not $content) { continue }
        $pattern = '(?<![\w:])' + [regex]::Escape($name) + '\s*\{'
        foreach ($m in [regex]::Matches($content, $pattern)) {
            $start = [Math]::Max(0, $m.Index - 60)
            $before = $content.Substring($start, $m.Index - $start)
            if ($before -match '(struct|impl|enum|fn|trait|union)\s+$') { continue }
            $lineNo = ($content.Substring(0, $m.Index) -split "`n").Count
            $sites += [pscustomobject]@{ File = (Split-Path $path -Leaf); Path = $path; Line = $lineNo }
        }
    }
    $report += [pscustomobject]@{ Struct = $name; N = $info.N; DefFile = (Split-Path $info.File -Leaf); Sites = $sites }
}

$report = $report | Sort-Object -Property { $_.Sites.Count } -Descending

# ---- 第 3 步：报告
Write-Host ""
Write-Host "=== 结构体加字段风险报告 ==="
Write-Host ""
Write-Host ("{0,-30} {1,-10} {2,-12} {3}" -f "Struct", "新增字段", "字面量点", "定义文件")
Write-Host ("-" * 80)
foreach ($r in $report) {
    $n = $r.Sites.Count
    $marker = if ($n -gt 0) { "[!] " } else { "    " }
    Write-Host ("{0}{1,-28} {2,-10} {3,-12} {4}" -f $marker, $r.Struct, $r.N, $n, $r.DefFile)
}

$totalSites = ($report | ForEach-Object { $_.Sites.Count } | Measure-Object -Sum).Sum
$riskyCount = @($report | Where-Object { $_.Sites.Count -gt 0 }).Count
Write-Host ""
Write-Host "合计: $($report.Count) 个加过字段的结构体 / $totalSites 处字面量构造点 / $riskyCount 个有风险"

if ($riskyCount -gt 0) {
    Write-Host ""
    Write-Host "=== 前 5 个高风险结构体 ==="
    foreach ($r in ($report | Where-Object { $_.Sites.Count -gt 0 } | Select-Object -First 5)) {
        Write-Host ""
        Write-Host "--- $($r.Struct)（$($r.Sites.Count) 处）---"
        foreach ($s in ($r.Sites | Select-Object -First 8)) { Write-Host "    $($s.File):$($s.Line)" }
        if ($r.Sites.Count -gt 8) { Write-Host "    ...（还有 $($r.Sites.Count - 8) 处）" }
    }
}

Write-Host ""
Write-Host "=== 怎么消除风险 ==="
Write-Host "  给结构体加 impl Default，字面量改成："
Write-Host "    Foo { existing_field: value, ..Default::default() }"
Write-Host "  这样以后加字段只改 Default 一处。"

if ($Strict -and $riskyCount -gt 0) {
    Write-Host ""
    Write-Host "[struct-guard] FAIL: $riskyCount 个结构体有字面量构造点"
    exit 1
}

Write-Host ""
Write-Host "[struct-guard] OK（报告模式）"
exit 0