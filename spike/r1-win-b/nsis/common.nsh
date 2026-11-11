; Shared part of the S6 probe hooks. The file under gen/ is written by the CI step before
; `tauri build`, as the wrapper would write it from packaging/helpers.d.
!include "${__FILEDIR__}\gen\helpers.nsh"

!macro R1SpikeLog text
  ReadEnvStr $9 R1SPIKE_S6_DIR
  ${If} $9 != ""
    FileOpen $8 "$9\hook.txt" a
    FileSeek $8 0 END
    FileWrite $8 "${text}$\r$\n"
    FileClose $8
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro R1SpikeLog "hook=postinstall generated_define=${R1SPIKE_GENERATED_AT_BUILD}"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro R1SpikeLog "hook=postuninstall"
!macroend
