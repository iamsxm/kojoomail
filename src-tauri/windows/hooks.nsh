!include "LogicLib.nsh"
!include "Win\RestartManager.nsh"

Var KojooMailStopError

; Remove the launch paths while replacing files. An MCP client can otherwise
; restart the bridge between taskkill and NSIS's File instruction. Keep the old
; images until extraction succeeds and scope all operations to this installation.
!define KOJOOMAIL_OLD_SUFFIX ".kojoomail-install-old"

!macro KojooMailProcessFunctions PREFIX
Function ${PREFIX}KojooMailStopBinary
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $4
  StrCpy $4 $0
  StrCpy $1 "$4${KOJOOMAIL_OLD_SUFFIX}"

  ; Recover a previous interrupted extraction before starting another attempt.
  IfFileExists "$1" 0 stage
    IfFileExists "$4" old_backup_exists
      ClearErrors
      Rename "$1" "$4"
      IfErrors failed stage
    old_backup_exists:
      !insertmacro RestartManager_ShutdownFile "$1" $2
      ClearErrors
      Delete "$1"
      IfErrors failed

  stage:
    IfFileExists "$4" 0 done
    DetailPrint "Closing $4"
    ClearErrors
    Rename "$4" "$1"
    IfErrors stop_before_rename stop_staged

  stop_before_rename:
    ; Some file holders do not grant FILE_SHARE_DELETE. Stop them before retrying
    ; the rename. Never extract if the old file cannot be moved.
    !insertmacro RestartManager_ShutdownFile "$4" $2
    ClearErrors
    Rename "$4" "$1"
    IfErrors failed

  stop_staged:
    ; Restart Manager finds console/stdio processes by their executable file.
    ; Do not call RmRestart: the bridge must stay stopped during extraction.
    !insertmacro RestartManager_ShutdownFile "$1" $2
    DetailPrint "Restart Manager: $4 (result $2)"
    StrCpy $3 30
  wait_for_release:
    ; Termination and release of the image mapping need not happen together.
    ; A write handle proves the executable is no longer mapped/running.
    ; OPEN_EXISTING avoids creating a missing file; this does not change bytes.
    System::Call 'kernel32::CreateFileW(w r1, i 0x40000000, i 7, p 0, i 3, i 0, p 0) p.r2'
    IntCmp $2 -1 still_locked
      System::Call 'kernel32::CloseHandle(p r2)'
      Goto done
  still_locked:
    Sleep 100
    IntOp $3 $3 - 1
    IntCmp $3 0 failed wait_for_release wait_for_release

  failed:
    StrCpy $KojooMailStopError "$4"
  done:
    Pop $4
    Pop $3
    Pop $2
    Pop $1
    Pop $0
FunctionEnd

Function ${PREFIX}KojooMailRestoreBinaries
  ; This runs before File/Delete instructions. Do not overwrite a new executable
  ; from an installation whose extraction already completed.
  IfFileExists "$INSTDIR\kojoomail.exe" +2
    Rename "$INSTDIR\kojoomail.exe${KOJOOMAIL_OLD_SUFFIX}" "$INSTDIR\kojoomail.exe"
  IfFileExists "$INSTDIR\Kojoo Mail.exe" +2
    Rename "$INSTDIR\Kojoo Mail.exe${KOJOOMAIL_OLD_SUFFIX}" "$INSTDIR\Kojoo Mail.exe"
  IfFileExists "$INSTDIR\kojoomail-mcp.exe" +2
    Rename "$INSTDIR\kojoomail-mcp.exe${KOJOOMAIL_OLD_SUFFIX}" "$INSTDIR\kojoomail-mcp.exe"
FunctionEnd

Function ${PREFIX}KojooMailPrepareBinaries
  StrCpy $KojooMailStopError ""
  Push "$INSTDIR\kojoomail.exe"
  Call ${PREFIX}KojooMailStopBinary
  StrCmp $KojooMailStopError "" 0 failed
  ; Historical installations used the product name as the main binary name.
  Push "$INSTDIR\Kojoo Mail.exe"
  Call ${PREFIX}KojooMailStopBinary
  StrCmp $KojooMailStopError "" 0 failed
  Push "$INSTDIR\kojoomail-mcp.exe"
  Call ${PREFIX}KojooMailStopBinary
  StrCmp $KojooMailStopError "" 0 failed
  Return

  failed:
    Call ${PREFIX}KojooMailRestoreBinaries
    DetailPrint "Cannot release executable: $KojooMailStopError"
    IfSilent no_dialog
      MessageBox MB_OK|MB_ICONSTOP "无法关闭或替换程序：$\r$\n$KojooMailStopError$\r$\n$\r$\n请关闭使用 Kojoo Mail MCP 的客户端后重试。如果程序以管理员身份运行，请也以管理员身份运行安装程序。"
    no_dialog:
    SetErrorLevel 2
    Abort "无法解除程序文件占用，已停止安装。"
FunctionEnd
!macroend

!insertmacro KojooMailProcessFunctions ""
!insertmacro KojooMailProcessFunctions "un."

!macro KojooMailDeleteOldBinaries
  Delete "$INSTDIR\kojoomail.exe${KOJOOMAIL_OLD_SUFFIX}"
  Delete "$INSTDIR\Kojoo Mail.exe${KOJOOMAIL_OLD_SUFFIX}"
  Delete "$INSTDIR\kojoomail-mcp.exe${KOJOOMAIL_OLD_SUFFIX}"
!macroend

!macro NSIS_HOOK_PREINSTALL
  Call KojooMailPrepareBinaries
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro KojooMailDeleteOldBinaries
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Call un.KojooMailPrepareBinaries
  !insertmacro KojooMailDeleteOldBinaries
!macroend
