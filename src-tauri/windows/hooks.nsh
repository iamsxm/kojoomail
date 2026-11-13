; 默认模板只对主程序做 Restart Manager 检查，独立运行的 MCP 代理会锁住安装文件；安装/卸载前按映像名强制结束两者。
!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog 'taskkill /F /T /IM kojoomail.exe'
  Pop $0
  nsExec::ExecToLog 'taskkill /F /T /IM kojoomail-mcp.exe'
  Pop $0
  Sleep 500
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog 'taskkill /F /T /IM kojoomail.exe'
  Pop $0
  nsExec::ExecToLog 'taskkill /F /T /IM kojoomail-mcp.exe'
  Pop $0
  Sleep 500
!macroend