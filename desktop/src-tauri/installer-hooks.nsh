; Tauri NSIS 自定义钩子。
; 1. 安装/卸载完成后强制刷新 Windows 图标缓存,解决"装了新版图标还是旧的"问题
;    (Windows shell 默认会缓存图标几小时)。
; 2. 全局开关 MUI_HEADERIMAGE_RIGHT:把安装向导顶部的 header 图从默认左侧移到右侧
;    (与 Clash Verge Rev 等多数 Tauri 应用一致;Tauri 在 MUI 配置之前 include 本文件,
;    所以放在最外层全局生效)。
; Tauri 安装脚本会按命名约定调用 NSIS_HOOK_POSTINSTALL / NSIS_HOOK_POSTUNINSTALL。

!define MUI_HEADERIMAGE_RIGHT

!macro NSIS_HOOK_POSTINSTALL
  ; -ClearIconCache 是 Win10/11 标准 API,无需重启资源管理器
  ExecWait '"$SYSDIR\ie4uinit.exe" -show'
  ExecWait '"$SYSDIR\ie4uinit.exe" -ClearIconCache'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ExecWait '"$SYSDIR\ie4uinit.exe" -ClearIconCache'
!macroend
