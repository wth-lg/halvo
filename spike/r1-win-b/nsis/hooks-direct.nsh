; S6 probe, variant direct: the hook inserts the template's running-app macro (Restart Manager
; inside) for the helper, in the same section where the template inserts it for the app.
!include "${__FILEDIR__}\common.nsh"

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro R1SpikeLog "hook=preuninstall variant=direct"
  ${If} ${Silent}
    !insertmacro R1SpikeLog "silent=1"
  ${EndIf}
  ExecWait '"$INSTDIR\${R1SPIKE_HELPER_EXE}" ctl stop' $7
  !insertmacro R1SpikeLog "ctl_stop_exit=$7"
  Sleep 1000
  !insertmacro CheckIfAppIsRunning "$INSTDIR\${R1SPIKE_HELPER_EXE}" "${PRODUCTNAME}"
  !insertmacro R1SpikeLog "after_running_app_macro=1"
!macroend
