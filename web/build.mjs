// esbuild bundler for the R9 web bridge: src/main.ts -> dist/main.js (+ static assets).
// The wasm-bindgen glue is bundled in; the .wasm binary is copied and loaded
// with an explicit relative URL (`import.meta.url`-based, emitted by the glue)
// so dist/ is self-contained — the same directory a wry host shell will point
// at (R10). Two-step build: `./build-wasm.sh` produces pkg/, then `npm run build`.
//
// Phase 5 §八.4 additions (PDF surface view):
//   - pdfjs-dist (npm dep, Apache-2.0) is bundled into main.js via the
//     dynamic `import('pdfjs-dist')` in pdf_view.ts (esbuild inlines it —
//     code splitting off by default, so the lazy import costs no extra file
//     but defers module execution).
//   - pdf.worker.min.mjs is copied explicitly to dist/ (pdf.js workerSrc
//     points at it via `new URL('./pdf.worker.min.mjs', import.meta.url)` —
//     the specifier is a variable in pdf_view.ts on purpose: esbuild's
//     literal `new URL('./x', import.meta.url)` asset analysis would try to
//     resolve it relative to src/ and fail; the explicit copy is the stable
//     path per the §八.4 build decision).
//   - web/pkg_pdf/ (mdc-pdf second wasm package — build-pdf-wasm.sh output)
//     is copied wholesale when present; absent = pdf_view display-only mode.
import * as esbuild from 'esbuild';
import { cpSync, existsSync, mkdirSync } from 'fs';

if (!existsSync('pkg/mdcore_wasm_bg.wasm')) {
  console.error('missing pkg/mdcore_wasm_bg.wasm — run `./build-wasm.sh` first (Rust -> WASM -> JS glue)');
  process.exit(1);
}

mkdirSync('dist', { recursive: true });

const entries = [
  { in: 'src/main.ts', out: 'dist/main.js' },
  { in: 'src/bench.ts', out: 'dist/bench.js' }, // 阶段1宿主选型：原型A基准页
];

for (const e of entries) {
  await esbuild.build({
    entryPoints: [e.in],
    outfile: e.out,
    bundle: true,
    format: 'esm', // wasm-bindgen web-target glue is an ES module
    target: 'es2022',
    minify: false,
    logLevel: 'info',
  });
}

cpSync('index.html', 'dist/index.html');
cpSync('bench.html', 'dist/bench.html');
cpSync('pkg/mdcore_wasm_bg.wasm', 'dist/mdcore_wasm_bg.wasm');

// ---- §八.4: pdf.js worker（显式拷贝——见文件头说明）----
// legacy 构建 worker（与 main 模块同族——modern worker 会用旧引擎缺的
// Map.getOrInsertComputed 等新 API，见 pdf_view.ts PdfjsModule 注释）
const PDFJS_WORKER = 'node_modules/pdfjs-dist/legacy/build/pdf.worker.min.mjs';
if (!existsSync(PDFJS_WORKER)) {
  console.error(`missing ${PDFJS_WORKER} — run \`npm install\` first (pdfjs-dist is a runtime dep)`);
  process.exit(1);
}
cpSync(PDFJS_WORKER, 'dist/pdf.worker.min.mjs');

// ---- §八.4: mdc-pdf 第二 wasm 包（pkg_pdf——缺席 = display-only 降级）----
if (existsSync('pkg_pdf/pdf_wasm.js')) {
  cpSync('pkg_pdf', 'dist/pkg_pdf', { recursive: true });
  console.log('pdf wasm pkg -> dist/pkg_pdf/');
} else {
  console.log('note: web/pkg_pdf absent (mdc-pdf-wasm pending) — pdf_view runs display-only');
}

console.log('build ok -> dist/');
