!include "LogicLib.nsh"

Name "Lunu"
OutFile "LunuInstaller.exe"
InstallDir "$PROGRAMFILES\Lunu"
RequestExecutionLevel admin

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Lunu"
SetOutPath "$INSTDIR"
File /r /x "installer" "..\*"

CreateDirectory "$SMPROGRAMS\Lunu"
CreateShortCut "$SMPROGRAMS\Lunu\Lunu.lnk" "$INSTDIR\bin\lunu.exe"
CreateShortCut "$SMPROGRAMS\Lunu\Uninstall Lunu.lnk" "$INSTDIR\Uninstall.exe"

ExecWait '"$SYSDIR\WindowsPowerShell\\v1.0\\powershell.exe" -NoProfile -ExecutionPolicy Bypass -Command "$p=[Environment]::GetEnvironmentVariable(''Path'',''User'');$b='''$INSTDIR\\bin''';if($p -notlike ''*''+$b+''*''){[Environment]::SetEnvironmentVariable(''Path'',$p+'';''+$b,''User'')}"'

ExecWait '"$INSTDIR\bin\lunu.exe" --help' $0
${If} $0 != 0
MessageBox MB_ICONEXCLAMATION "Lunu installed, but validation failed. Check PATH and dependencies."
${EndIf}

WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
Delete "$SMPROGRAMS\Lunu\Lunu.lnk"
Delete "$SMPROGRAMS\Lunu\Uninstall Lunu.lnk"
RMDir "$SMPROGRAMS\Lunu"

ExecWait '"$SYSDIR\WindowsPowerShell\\v1.0\\powershell.exe" -NoProfile -ExecutionPolicy Bypass -Command "$p=[Environment]::GetEnvironmentVariable(''Path'',''User'');$b='''$INSTDIR\\bin''';if($p){$p=$p -replace [regex]::Escape('';''+$b),'''';$p=$p -replace [regex]::Escape($b+'';''),'''';[Environment]::SetEnvironmentVariable(''Path'',$p,''User'')}"'

RMDir /r "$INSTDIR"
SectionEnd
