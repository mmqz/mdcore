# -*- coding: utf-8 -*-
import sys, io, os, json, base64, subprocess, time, socket
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

JS = """(b64) => {
  const bin = atob(b64); const arr = new Uint8Array(bin.length);
  for (let i=0;i<bin.length;i++) arr[i]=bin.charCodeAt(i);
  const r = window.__importDocx(arr);
  return {keys: Object.keys(r), raw: JSON.stringify(r).slice(0,1500), md: String(window.__md()).slice(0,800), blocks: window.__blockCount()};
}"""
JSX = """(b64) => {
  const bin = atob(b64); const arr = new Uint8Array(bin.length);
  for (let i=0;i<bin.length;i++) arr[i]=bin.charCodeAt(i);
  const r = window.__importXlsx(arr);
  return {keys: Object.keys(r), raw: JSON.stringify(r).slice(0,1500), md: String(window.__md()).slice(0,800)};
}"""
CASES = [
  (r"E:\Code\ai\md\audit\w2\word-complex.docx", JS),
  (r"E:\Code\ai\md\audit\w2\word-merged.docx", JS),
  (r"E:\Code\ai\md\audit\w2\excel-complex.xlsx", JSX),
]
try:
    with sync_playwright() as p:
        b = p.chromium.launch(executable_path=CHROME, headless=True)
        pg = b.new_page(viewport={"width":1400,"height":900})
        pg.goto("http://127.0.0.1:8123/index.html", wait_until="networkidle")
        pg.wait_for_timeout(4500)
        for path, js in CASES:
            name = os.path.basename(path)
            b64 = base64.b64encode(open(path, "rb").read()).decode()
            r = pg.evaluate(js, b64)
            print("=" * 70)
            print(name)
            print("result keys:", r.get("keys"))
            print("raw:", r.get("raw", "")[:900])
            print("md:", r.get("md", "")[:700])
            print("blocks:", r.get("blocks"))
        b.close()
finally:
    if srv: srv.terminate()