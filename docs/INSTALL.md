# PDFium installation

The desktop PDF reader uses the native PDFium library. PDFium is a runtime dependency and is not bundled by this repository. Download a release from the [pdfium-binaries releases](https://github.com/bblanchon/pdfium-binaries/releases) page. Choose the archive matching your operating system and CPU architecture (for example `win-x64` on 64-bit Windows; do **not** pick a `v8` variant).

## How the application locates PDFium

The loader tries locations in this order, and the error message lists every path it attempted:

1. The `MDC_PDFIUM_LIB` environment variable, set to the full path of the library file (`pdfium.dll` / `libpdfium.so`).
2. The directory of the application executable (`pdfium.dll` / `libpdfium.so` next to the exe).
3. Repository development fetch locations (only present in development checkouts).
4. The system dynamic-library search path.

Placing the library next to the executable is the recommended default; `MDC_PDFIUM_LIB` overrides everything.

## Windows

1. Download and extract the matching Windows archive.
2. Copy `bin/pdfium.dll` beside the application executable, or set `MDC_PDFIUM_LIB` to the full path of `pdfium.dll`.
3. Restart the application, then open a PDF to verify that pages render.

PowerShell example for the current terminal session:

```powershell
$env:MDC_PDFIUM_LIB = "C:\path\to\pdfium.dll"
```

## Linux and macOS

Download the matching release archive and either place the native library (`libpdfium.so` on Linux) beside the application executable, install it in a location the dynamic loader can find, or set `MDC_PDFIUM_LIB` to the full library path before starting the application.

## Troubleshooting

Run the deployment self-check to diagnose without opening a PDF:

```
mang-shell --check-pdfium
```

The command prints every layer of the resolution order — layer name, expected path, whether the file exists (and its size), and the loader's own error when a load was attempted — then either confirms the bound library (exit code `0`) or prints an actionable fix guide: what is missing, where to download it, where to put it (with the concrete exe-adjacent directory), and how to verify (exit code `3` means not bound).

If PDFium cannot be loaded, the error reports the library name and every path that was tried. Confirm that the file exists, that its architecture matches the application (a 64-bit application needs the x64 library), and that any platform runtime dependencies are installed. Setting `MDC_PDFIUM_LIB` directly to the library file removes all search ambiguity — if that works but the exe-adjacent placement does not, check that the file name matches exactly (`pdfium.dll` on Windows, `libpdfium.so` on Linux).
