; NSIS 安装器模板（R24-c；实体验证留 R24-d）
; 注入：mang {{VERSION}}（构建脚本从 mdcore::VERSION 取值）
; 方言：NSIS 3.x；宿主壳 = tao+wry（Windows 下 WebView2 运行时）
; 注意：可执行名/安装目录用自研命名域 mdcore（TRADEMARK_RULES §1）；
;       mang（显示名）待定名后注入——checklist A4。

!define APPEXEC "mdcore"
!define APPNAME "mang"
!define APPVERSION "{{VERSION}}"
!define APPID "dev.mdcore.editor"
; WebView2 Evergreen 引导（wry Windows 运行时面；R24-d 实测后锁版本）
!define WEBVIEW2_BOOTSTRAPPER "https://go.microsoft.com/fwlink/p/?LinkId=2124703"

Name "${APPNAME} ${APPVERSION}"
OutFile "mdcore-setup-${APPVERSION}.exe"
Unicode true
InstallDir "$PROGRAMFILES64\${APPEXEC}"
InstallDirRegKey HKLM "Software\${APPID}" "InstallDir"
RequestExecutionLevel admin

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "${APPNAME} (required)"
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "..\..\target\release\mdcore.exe"
  File /r "..\..\web\dist"

  ; 注册表面：开始菜单 + 默认程序（checklist D2）
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\${APPEXEC}.exe"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\Uninstall ${APPNAME}.lnk" "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\${APPID}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
                 "DisplayName" "${APPNAME}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
                 "DisplayVersion" "${APPVERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}" \
                 "UninstallString" "$INSTDIR\uninstall.exe"

  ; .md 文件关联（HKCR 面；R24-d 冒烟验证默认程序注册）
  WriteRegStr HKCR ".md" "" "MarkdownFile"
  WriteRegStr HKCR "MarkdownFile\shell\open\command" "" '"$INSTDIR\${APPEXEC}.exe" "%1"'

  ; WebView2 Runtime 检测 + 引导（无运行时则下载安装器并执行）
  Call EnsureWebView2

  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Function EnsureWebView2
  ReadRegStr $0 HKLM "Software\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  IfErrors 0 +3
    DetailPrint "WebView2 Runtime 缺失，引导安装（Evergreen）……"
    ExecWait '"$TEMP\wv2setup.exe" /install /silent'
  Return
FunctionEnd

Section "Uninstall"
  Delete "$INSTDIR\${APPEXEC}.exe"
  RMDir /r "$INSTDIR"
  RMDir /r "$SMPROGRAMS\${APPNAME}"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}"
  DeleteRegKey HKLM "Software\${APPID}"
  DeleteRegKey HKCR "MarkdownFile"
SectionEnd
