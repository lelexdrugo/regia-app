; Hooks of the NSIS installer: bundle > windows > nsis > installerHooks in tauri.conf.json.
;
; The CLI ships next to the app, in the install folder, and the installer is per user. So the folder goes
; at the end of the user's PATH, and comes off it at uninstall; an update finds it there and leaves it.
; PowerShell edits the value because it reads and writes it unexpanded, as REG_EXPAND_SZ, so the
; %VARIABLES% in it survive, and with no length limit: a string of NSIS stops at 1024 characters, and a
; longer PATH would come back empty. The folder reaches PowerShell through the environment, so no
; character of it is ever parsed as code.

!define USER_PATH_ADD "$$d = $$env:INSTALL_DIR.TrimEnd('\'); $$k = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment'); $$p = [string]$$k.GetValue('Path', '', 'DoNotExpandEnvironmentNames'); $$e = @($$p -split ';' | Where-Object { $$_ }); if (-not ($$e | Where-Object { $$_.TrimEnd('\') -eq $$d })) { $$k.SetValue('Path', (($$e + $$d) -join ';'), 'ExpandString') }; $$k.Close()"

!define USER_PATH_REMOVE "$$d = $$env:INSTALL_DIR.TrimEnd('\'); $$k = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment'); $$p = [string]$$k.GetValue('Path', '', 'DoNotExpandEnvironmentNames'); $$q = @($$p -split ';' | Where-Object { $$_ -and $$_.TrimEnd('\') -ne $$d }) -join ';'; if ($$q -ne $$p) { $$k.SetValue('Path', $$q, 'ExpandString') }; $$k.Close()"

!macro EDIT_USER_PATH SCRIPT
  Push $0
  System::Call 'Kernel32::SetEnvironmentVariable(t "INSTALL_DIR", t "$INSTDIR")'
  nsExec::ExecToLog `"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "${SCRIPT}"`
  Pop $0
  Pop $0
  ; Explorer, and the shells it starts, read the new PATH only after this broadcast (HWND_BROADCAST,
  ; WM_SETTINGCHANGE). The timeout keeps a window that does not answer from holding the installer.
  SendMessage 0xFFFF 0x1A 0 "STR:Environment" /TIMEOUT=500
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro EDIT_USER_PATH "${USER_PATH_ADD}"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro EDIT_USER_PATH "${USER_PATH_REMOVE}"
!macroend
