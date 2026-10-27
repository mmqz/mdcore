# -*- coding: utf-8 -*-
import sys, io, os, subprocess, time, socket
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright
CHROME = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
WEB = r"E:\Code\ai\md\audit\mdcore\web"

def port_open(p=8123):
    s = socket.socket(); s.settimeout(0.5)
    try: return s.connect_ex(("127.0.0.1", p)) == 0
    finally: s.close()

srv = None
if not port_open():
    srv = subprocess.Popen([sys.executable, "-m", "http.server", "8123", "--directory", "dist"],
                           cwd=WEB, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(40):
        if port_open(): break
        time.sleep(0.25)
print("server up:", port_open())

md = "# 项目技术方案书\n\n## 第一章 概述\n\n本项目含**智能文档平台**与*需求解析*两项内容。\n\n- 支持 Word 导入\n- 支持 Excel 导入\n\n### 1.1 表格\n\n| 阶段 | 时间 |\n|---|---|\n| 一期 | 2026 Q1 |\n\n## 第二章 报价\n\n总计人民币 **壹佰贰拾万元整**。\n"
try:
    with sync_playwright() as p:
        b = p.chromium.launch(executable_path=CHROME, headless=True)
        ctx = b.new_context(viewport={"width":1400,"height":900}, accept_downloads=True)
        pg = ctx.new_page()
        errs = []
        pg.on("pageerror", lambda e: errs.append(str(e)))
        pg.goto("http://127.0.0.1:8123/index.html", wait_until="networkidle")
        pg.wait_for_timeout(5000)
        pg.evaluate("(m) => window.__inject(m)", md)
        pg.wait_for_timeout(2000)
        print("blocks:", pg.evaluate("() => window.__blockCount()"))
        api = pg.evaluate("() => Object.keys(window).filter(k=>/export|styled|typo|geom/i.test(k))")
        print("export api:", api)
        with pg.expect_download(timeout=30000) as dl:
            pg.click("#docx-export-btn")
        out = r"E:\Code\ai\md\audit\tr2-export.docx"
        dl.value.save_as(out)
        print("docx bytes:", os.path.getsize(out))
        print("errs:", errs[:3])
        b.close()
finally:
    if srv: srv.terminate()