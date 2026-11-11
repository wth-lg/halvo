; S6 probe, variant plugin: the hook calls the installer plugin's process functions directly.
!include "${__FILEDIR__}\common.nsh"

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro R1SpikeLog "hook=preuninstall variant=plugin"
  ${If} ${Silent}
    !insertmacro R1SpikeLog "silent=1"
  ${EndIf}
  ExecWait '"$INSTDIR\${R1SPIKE_HELPER_EXE}" ctl stop' $7
  !insertmacro R1SpikeLog "ctl_stop_exit=$7"
  Sleep 1000
  nsis_tauri_utils::FindProcessCurrentUser "${R1SPIKE_HELPER_EXE}"
  Pop $6
  !insertmacro R1SpikeLog "find_result=$6"
  ${If} $6 = 0
    nsis_tauri_utils::KillProcessCurrentUser "${R1SPIKE_HELPER_EXE}"
    Pop $6
    !insertmacro R1SpikeLog "kill_result=$6"
  ${EndIf}
!macroend
