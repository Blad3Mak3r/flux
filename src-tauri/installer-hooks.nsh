LangString FluxInstallDirectory 1033 "Flux Timecode must be installed in $PROGRAMFILES64\${PRODUCTNAME}."
LangString FluxInstallDirectory 1034 "Flux Timecode debe instalarse en $PROGRAMFILES64\${PRODUCTNAME}."

!macro NSIS_HOOK_PREINSTALL
  ; Enforce the same path for both interactive installs and updater invocations.
  StrCmp $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}" +3
  MessageBox MB_OK|MB_ICONSTOP "$(FluxInstallDirectory)" /SD IDOK
  Abort
!macroend
