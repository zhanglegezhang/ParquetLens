; Right-click integration removed. Clean only a legacy verb owned by this install.
!macro PARQUETLENS_REMOVE_LEGACY_VERB
  ReadRegStr $R0 HKCU "Software\Classes\SystemFileAssociations\.parquet\shell\ParquetLens\command" ""
  ${If} $R0 == '$\"$INSTDIR\ParquetLens.exe$\" $\"%1$\"'
    DeleteRegKey HKCU "Software\Classes\SystemFileAssociations\.parquet\shell\ParquetLens"
    DeleteRegKey /ifempty HKCU "Software\Classes\SystemFileAssociations\.parquet\shell"
    DeleteRegKey /ifempty HKCU "Software\Classes\SystemFileAssociations\.parquet"
  ${EndIf}
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro PARQUETLENS_REMOVE_LEGACY_VERB
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Explicit silent equivalent of the existing uninstall data checkbox.
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/DELETEAPPDATA" $R1
  ${IfNot} ${Errors}
    StrCpy $DeleteAppDataCheckboxState 1
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro PARQUETLENS_REMOVE_LEGACY_VERB
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ; Fixed application-owned files only; never recursively remove the install folder.
    Delete "$LOCALAPPDATA\ParquetLens\settings.json"
    Delete "$LOCALAPPDATA\ParquetLens\cache\*"
    RMDir "$LOCALAPPDATA\ParquetLens\cache"
    RMDir "$LOCALAPPDATA\ParquetLens"
  ${EndIf}
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
