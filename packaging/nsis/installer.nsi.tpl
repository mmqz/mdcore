; NSIS 安装器模板（#228 R4 重构：宿主壳 = mang-shell 主线，GPU 自绘渲染/DirectX 后端）
; 注入（packaging/nsis/build-installer.py 渲染，残留占位符/不一致即失败
;   ——链路可验证，断链 1 修复）：
;   {{VERSION}}      根 Cargo.toml [workspace.package].version（=
;                    mdcore::VERSION 单源，r24 锁背书；断链 1）
;   {{OUT_FILE}}     安装包产物绝对路径（build/nsis/mdcore-setup-<ver>.exe）
;   {{PAYLOAD_EXE}}  mdcore.exe 绝对路径（= mang-shell.exe staging 重命名）
;   {{PAYLOAD_DLL}}  pdfium.dll 绝对路径（定位序取件位 staging）
; 路径面铁律：NSIS 的 File/OutFile 按脚本目录而非调用 CWD 解析（探针
;   实证：脚本在 X 目录、cwd=Y，File "a/f" 找 X/a/f、OutFile 落 X/——
;   BUILD-VERIFY §3）；旧模板 "..\.." 反跳正是断链 3 的根源。故本模板
;   一律用构建器注入的绝对路径，任何调用目录/平台零歧义。
; 显示名 mang（A4 定名 2026-10-27，packaging/README 定名记录）；可执行名/
;   安装目录用自研命名域 mdcore（TRADEMARK_RULES §1）；APPID 恒
;   dev.mdcore.editor（r24_packaging.rs 门锁锚）。
; 载荷语义：pdfium.dll 与 mdcore.exe 同目录安装 = 定位序 exe-dir 候选
;   （#223/#224）；安装后自检复用 --check-pdfium（退出码 0=已绑定 /
;   3=未绑定）。
; WebView2 面已移除（断链 2 处置 = 面移除）：主线壳 mang-shell 走
;   GPU 自绘渲染（DirectX 后端），无 WebView 运行时依赖；旧 tao+wry 壳（mdshell-native）
;   为 Linux GTK 面（windows-check.yml 排除注释同源），不随 Windows
;   安装包分发。处置记录见 BUILD-VERIFY §3。

!define APPEXEC "mdcore"
!define APPNAME "mang"
!define APPVERSION "{{VERSION}}"
!define APPID "dev.mdcore.editor"

Name "${APPNAME} ${APPVERSION}"
OutFile "{{OUT_FILE}}"
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
  ; 载荷（构建器注入的绝对路径——见模板头「路径面铁律」）
  File "{{PAYLOAD_EXE}}"
  File "{{PAYLOAD_DLL}}"

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

  ; 安装后部署自检（#224 能力复用）：pdfium 定位序逐层探针 + 绑定走查。
  ; 退出码合同：0 = 已绑定 / 3 = 未绑定（mang-shell main.rs 与
  ; mdc-pdf deploy_check 同源）。失败不回滚安装（Markdown/Word 面不依赖
  ; pdfium）——弹窗给复跑命令，报告全文进安装日志（详情页可见）。
  Call DeploySmoke

  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

Function DeploySmoke
  DetailPrint "部署自检：${APPEXEC}.exe --check-pdfium ..."
  nsExec::ExecToLog '"$INSTDIR\${APPEXEC}.exe" --check-pdfium'
  Pop $0
  ; 退出码 0 = pdfium 已绑定；其余（含 3=未绑定、负值=无法启动）走告警。
  IntCmp $0 0 smoke_ok smoke_fail smoke_fail
smoke_fail:
  MessageBox MB_ICONEXCLAMATION "部署自检未通过（退出码 $0）。$\n$\nPDF 面暂不可用（Markdown / Word 面不受影响）。$\n修复指引：在命令行运行安装目录（$INSTDIR）下的 ${APPEXEC}.exe --check-pdfium，查看逐层探针与修复指引。"
smoke_ok:
  DetailPrint "部署自检退出码：$0"
FunctionEnd

Section "Uninstall"
  Delete "$INSTDIR\${APPEXEC}.exe"
  Delete "$INSTDIR\pdfium.dll"
  RMDir /r "$INSTDIR"
  RMDir /r "$SMPROGRAMS\${APPNAME}"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPID}"
  DeleteRegKey HKLM "Software\${APPID}"
  DeleteRegKey HKCR "MarkdownFile"
SectionEnd
