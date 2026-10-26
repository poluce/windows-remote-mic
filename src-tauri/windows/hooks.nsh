; Remote Mic NSIS installer hooks: set up / tear down the virtual HID keyboard
; driver (WinUHid UMDF package) so that end users do not have to run anything by
; hand.
;
; The bundle referenced below is produced by scripts\prepare-vhid-bundle.ps1 and
; lives in windows\driver next to this file, which is exactly what Tauri's
; ${__FILEDIR__} resolves to at the top level of a hook file.
;
; The heavy lifting is done by the same PowerShell scripts the developer flow
; uses, so there is a single implementation of driver install/uninstall.
;
; A missing bundle is not a compile error (File /nonfatal) so that a plain
; `npm run tauri build` still works during development. The release workflow
; verifies the bundle explicitly, and the runtime guards below keep the
; installer from invoking scripts that were not packaged.
;
; Messages here are ASCII on purpose: NSIS script encoding is not guaranteed to
; round-trip non-ASCII text.

!define VHID_SRC "${__FILEDIR__}\driver"
!define VHID_DST "$INSTDIR\vhid"

; VB-CABLE 助手脚本（不是 VB-CABLE 本体：其 EULA 禁止把它集成进别的安装流程）。
; 用户之后在应用里点「下载并安装 VB-CABLE」时，用这里的脚本从官网下载、
; 校验 SHA-256，再启动官方安装程序（需管理员权限，安装后需重启）。
!define VBCABLE_SRC "${__FILEDIR__}\vb-cable"
!define VBCABLE_DST "$INSTDIR\scripts"

; Copy the driver bundle into the install directory so the app can re-run the
; installer later as an in-app repair action.
!macro RemoteMicVhidExtract
  SetOutPath "${VHID_DST}"
  File /nonfatal /r "${VHID_SRC}\*.*"
  SetOutPath "$INSTDIR"
!macroend

; Run one of the bundled driver scripts. The script re-launches itself elevated
; (and waits) when the current process is not an administrator.
!macro RemoteMicVhidRunScript SCRIPT WHAT
  DetailPrint "${WHAT}..."
  nsExec::ExecToLog 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "${VHID_DST}\${SCRIPT}" -DriverDir "${VHID_DST}"'
  Pop $0
  DetailPrint "${WHAT} finished (exit $0)"
!macroend

!macro RemoteMicVbCableExtract
  SetOutPath "${VBCABLE_DST}"
  File /nonfatal "${VBCABLE_SRC}\install-vb-cable.ps1"
  File /nonfatal "${VBCABLE_SRC}\vb-cable.sha256"
  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro RemoteMicVhidExtract
  !insertmacro RemoteMicVbCableExtract
  IfFileExists "${VHID_DST}\install-winuhid.ps1" 0 RemoteMicVhidSkipInstall
    !insertmacro RemoteMicVhidRunScript "install-winuhid.ps1" "Installing virtual HID keyboard driver"
  RemoteMicVhidSkipInstall:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Best-effort cleanup of the local agent API token file for the current user.
  Delete "$LOCALAPPDATA\RemoteMic\RC003\agent-api.json"
  ; Use labels rather than relative jumps: the macro above expands to several
  ; instructions, so "+2" would land inside it.
  IfFileExists "${VHID_DST}\uninstall-winuhid.ps1" 0 RemoteMicVhidSkipUninstall
    !insertmacro RemoteMicVhidRunScript "uninstall-winuhid.ps1" "Removing virtual HID keyboard driver"
  RemoteMicVhidSkipUninstall:
!macroend
