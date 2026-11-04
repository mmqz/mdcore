# PDFium installation

The desktop PDF reader uses the native PDFium library. PDFium is a runtime dependency and is not bundled by this repository. Download a release from the [pdfium-binaries releases](https://github.com/bblanchon/pdfium-binaries/releases) page. Choose the archive matching your operating system and CPU architecture.

## Windows

1. Download and extract the matching Windows archive.
2. Copy `bin/pdfium.dll` beside the application executable, or set `MDC_PDFIUM_LIB` to the full path of `pdfium.dll`.
3. Restart the application, then open a PDF to verify that pages render.

PowerShell example for the current terminal session:

```powershell
$env:MDC_PDFIUM_LIB = "C:\path\to\pdfium.dll"
```

## Linux and macOS

Download the matching release archive and install its native library (`libpdfium.so` on Linux or the platform library on macOS) in a location the dynamic loader can find. Alternatively, set `MDC_PDFIUM_LIB` to the full library path before starting the application.

## Troubleshooting

If PDFium cannot be loaded, the error reports the library name and paths that were tried. Confirm that the file exists, that its architecture matches the application, and that any platform runtime dependencies are installed. You can also set `MDC_PDFIUM_LIB` directly to the library file, restart the application, and retry opening a PDF.
