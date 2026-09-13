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

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro RemoteMicVhidExtract
  IfFileExists "${VHID_DST}\install-winuhid.ps1" 0 RemoteMicVhidSkipInstall
    !insertmacro RemoteMicVhidRunScript "install-winuhid.ps1" "Installing virtual HID keyboard driver"
  RemoteMicVhidSkipInstall:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Use labels rather than relative jumps: the macro above expands to several
  ; instructions, so "+2" would land inside it.
  IfFileExists "${VHID_DST}\uninstall-winuhid.ps1" 0 RemoteMicVhidSkipUninstall
    !insertmacro RemoteMicVhidRunScript "uninstall-winuhid.ps1" "Removing virtual HID keyboard driver"
  RemoteMicVhidSkipUninstall:
!macroend
