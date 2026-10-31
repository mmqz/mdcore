# ocr-tessdata —— S2-4 OCR 语言包（本地携带目录）

**tesseract 语言包注入目录**（`MDC_OCR_TESSDATA` 指向此处）。

## 为什么本地携带

沙盒无 root（`apt-get install tesseract-ocr-chi-sim` 不可行）——从
tessdata 官方仓取 `tessdata_fast` 构件到本目录，测试经
`MDC_OCR_TESSDATA` 注入（`pdff4_ocr.rs::setup_tessdata` 自动发现）。

## 纪律（对齐 large-file / pdf-organize-corpus 先例）

- `*.traineddata` 二进制**不入库**（chi_sim ≈ 2.4MB > 2MB 纪律）——
  见根 .gitignore；
- 取件脚本 + SHA256 pin 入库（版本闭环——升级 = 改 pin + 重取）。

## 取件

```bash
bash fetch_tessdata.sh   # chi_sim（tessdata_fast）+ 系统 eng/osd 复制
```

## 验证

```bash
TESSDATA_PREFIX=$(pwd) tesseract --list-langs
# 期望：chi_sim / eng / osd 三语言
```
