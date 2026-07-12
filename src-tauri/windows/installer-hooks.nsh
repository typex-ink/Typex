; Fresh per-user installs use the conventional Programs directory (ADR-26).
; Tauri has already restored an existing path or honored /D= before this hook.
!macro NSIS_HOOK_PREINSTALL
  ${If} $INSTDIR == "$LOCALAPPDATA\${PRODUCTNAME}"
    StrCpy $0 ""
    ReadRegStr $0 HKCU "${UNINSTKEY}" "UninstallString"
    ${If} $0 == ""
      StrCpy $0 ""
      ReadRegStr $0 HKCU "${MANUPRODUCTKEY}" ""
      ${If} $0 == ""
        StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${PRODUCTNAME}"
        SetOutPath "$INSTDIR"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend
