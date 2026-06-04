; Tauri NSIS 自定义钩子:安装/卸载完成后强制刷新 Windows 图标缓存,
; 解决"装了新版图标还是旧的"问题(Windows shell 默认会缓存图标几小时)。
; Tauri 安装脚本会按命名约定调用 NSIS_HOOK_POSTINSTALL / NSIS_HOOK_POSTUNINSTALL。

!macro NSIS_HOOK_POSTINSTALL
  ; -ClearIconCache 是 Win10/11 标准 API,无需重启资源管理器
  ExecWait '"$SYSDIR\ie4uinit.exe" -show'
  ExecWait '"$SYSDIR\ie4uinit.exe" -ClearIconCache'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ExecWait '"$SYSDIR\ie4uinit.exe" -ClearIconCache'
!macroend
