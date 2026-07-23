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

  ; Keep normal per-user installs unelevated. Protected legacy/custom paths
  ; relaunch the same installer once through UAC, preserving updater args.
  ClearErrors
  GetTempFileName $R8 "$INSTDIR"
  ${IfNot} ${Errors}
    Delete "$R8"
    ${IfNot} ${Errors}
      Goto typex_install_dir_writable
    ${EndIf}
  ${EndIf}

  ClearErrors
  ${GetOptions} $CMDLINE "/TYPEX-ELEVATED" $R9
  ${IfNot} ${Errors}
    MessageBox MB_ICONSTOP|MB_OK "Typex cannot write to the selected install directory, even with administrator permission."
    SetErrorLevel 5
    Quit
  ${EndIf}

  ${GetParameters} $R9
  StrCpy $R9 "/TYPEX-ELEVATED $R9 /D=$INSTDIR"
  ClearErrors
  ExecShell "runas" "$EXEPATH" "$R9"
  ${If} ${Errors}
    SetErrorLevel 1223
  ${Else}
    SetErrorLevel 0
  ${EndIf}
  Quit

  typex_install_dir_writable:
!macroend
