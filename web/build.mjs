// esbuild bundler for the R9 web bridge: src/main.ts -> dist/main.js (+ static assets).
// The wasm-bindgen glue is bundled in; the .wasm binary is copied and loaded
// with an explicit relative URL (`import.meta.url`-based, emitted by the glue)
// so dist/ is self-contained — the same directory a wry host shell will point
// at (R10). Two-step build: `./build-wasm.sh` produces pkg/, then `npm run build`.
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
console.log('build ok -> dist/');
