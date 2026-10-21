; Hooks of the Quirewise installer (Tauri NSIS). The installer and uninstaller run for the current
; user, so the Explorer menu the app may have added lives in HKCU and is removed with the app.

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Classes\SystemFileAssociations\.pdf\shell\Quirewise"
  DeleteRegKey HKCU "Software\Classes\Quirewise.PdfMenu"
  DeleteRegKey HKCU "Software\Classes\Applications\quirewise.exe"
!macroend
