; Halvo's hooks of the NSIS template of the Tauri bundler (tauri-cli 2.12.4, installer.nsi),
; named by bundle.windows.nsis.installerHooks in src-tauri/tauri.windows.conf.json; the section
; "Windows installer" of packaging/README.md says what they do. The template inserts each macro by
; its name: NSIS_HOOK_PREINSTALL in Section Install before its own CheckIfAppIsRunning and the copy
; of the files, NSIS_HOOK_POSTINSTALL at its end, NSIS_HOOK_PREUNINSTALL in Section Uninstall
; before its own CheckIfAppIsRunning and before any file or value goes, and
; NSIS_HOOK_POSTUNINSTALL at its end, behind the template's own removal of the folders of the app's
; id where the box is ticked. The hooks use $R4 to $R9, which CheckIfAppIsRunning leaves alone.

!define HALVO_SENTENCE_RUNTIME "Halvo cannot open its window yet: the Microsoft Edge WebView2 Runtime is not on this computer. This installer downloads nothing. Install the runtime from Microsoft, then start Halvo."
!define HALVO_SENTENCE_RUNNING "Halvo is still running, so nothing was removed. Quit Halvo from its icon near the clock, then run the uninstaller again."
!define HALVO_SENTENCE_RUNNING_INSTALL "Halvo is still running, so nothing was installed. Quit Halvo from its icon near the clock, then run the installer again."
!define HALVO_SENTENCE_NOT_DELETED "Halvo could not delete everything that it has stored, so the uninstaller stopped and Halvo is still installed. Start Halvo, choose Delete everything, then run the uninstaller again."
!define HALVO_SENTENCE_FOLDER_STAYS "Halvo was uninstalled, but a folder of its own window could not be removed: {folder}. It can hold what the window showed. Remove it yourself."
; The file in the install directory that a silent install writes sentence (1) into.
!define HALVO_RUNTIME_FILE "WEBVIEW2-RUNTIME-MISSING.txt"
; The switch of the uninstaller that ticks the box, also for a silent uninstall (/S).
!define HALVO_DELETE_SWITCH "/DELETEALL"
; The longest wait, in milliseconds, for halvo.exe and the helpers to be free, and for the folder
; of the app's id to go.
!define HALVO_WAIT_MS 20000

; Sets $R4 to NAME where $R4 is empty and the file NAME of the install directory stands and cannot
; be opened for writing, which Windows refuses while a process runs from it.
!macro HALVO_HELD NAME
  ${If} $R4 == ""
  ${AndIf} ${FileExists} "$INSTDIR\${NAME}"
    ClearErrors
    FileOpen $R5 "$INSTDIR\${NAME}" a
    ${If} ${Errors}
      StrCpy $R4 "${NAME}"
    ${Else}
      FileClose $R5
    ${EndIf}
  ${EndIf}
!macroend

; Before an uninstall and before an install over a copy: first the capture service's stop over
; its pipe (exit 69, where none runs, is no fault), then the app's end by the template's own step,
; then a wait of at most 20 s until no process runs from halvo.exe or a helper of
; packaging/helpers.d in the install directory. A file still held aborts with SENTENCE, sentence
; (2) before an uninstall and sentence (6) before an install, before any file or value goes.
!macro HALVO_STOP SENTENCE
  ${If} ${FileExists} "$INSTDIR\halvo-capture.exe"
    nsExec::Exec '"$INSTDIR\halvo-capture.exe" ctl stop'
    Pop $R4
  ${EndIf}
  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  StrCpy $R6 0
  ${Do}
    StrCpy $R4 ""
    !insertmacro HALVO_HELD "${MAINBINARYNAME}.exe"
    !insertmacro HALVO_HELD "halvo-analysis.exe"
    !insertmacro HALVO_HELD "halvo-capture.exe"
    !insertmacro HALVO_HELD "halvo-owner.exe"
    !insertmacro HALVO_HELD "halvo-runner.exe"
    ${If} $R4 == ""
    ${OrIf} $R6 >= ${HALVO_WAIT_MS}
      ${Break}
    ${EndIf}
    Sleep 250
    IntOp $R6 $R6 + 250
  ${Loop}
  ${If} $R4 != ""
    MessageBox MB_ICONSTOP|MB_OK "${SENTENCE}" /SD IDOK
    Abort "${SENTENCE}"
  ${EndIf}
!macroend

; Sets $R9 to the default data directory that halvo-capture.exe of the install directory names by
; its form --default-data-dir, or to "" where the form fails or names none. The form's output goes
; through a file and is read as UTF-8, since nsExec reads output in the ANSI code page.
!macro HALVO_DATA_DIR_OF_THE_PROGRAM
  StrCpy $R9 ""
  InitPluginsDir
  Delete "$PLUGINSDIR\halvo-data-dir.txt"
  nsExec::Exec '"$SYSDIR\cmd.exe" /d /c ""$INSTDIR\halvo-capture.exe" --default-data-dir >"$PLUGINSDIR\halvo-data-dir.txt""'
  Pop $R4
  ${If} $R4 == "0"
    ClearErrors
    FileOpen $R5 "$PLUGINSDIR\halvo-data-dir.txt" r
    ${IfNot} ${Errors}
      FileSeek $R5 0 END $R6
      FileSeek $R5 0 SET
      ${If} $R6 > 1
      ${AndIf} $R6 < 4096
        IntOp $R7 $R6 + 1
        System::Alloc $R7
        Pop $R7
        System::Call 'kernel32::ReadFile(p R5, p R7, i R6, *i .R8, p 0) i .R4'
        ${If} $R4 <> 0
        ${AndIf} $R8 = $R6
          System::Call 'kernel32::MultiByteToWideChar(i 65001, i 8, p R7, i -1, w .R8, i ${NSIS_MAX_STRLEN}) i .R4'
          ${If} $R4 > 2
            StrCpy $R4 $R8 1 -1
            ${If} $R4 == "$\n"
              StrCpy $R9 $R8 -1
            ${EndIf}
          ${EndIf}
        ${EndIf}
        System::Free $R7
      ${EndIf}
      FileClose $R5
    ${EndIf}
  ${EndIf}
!macroend

; With the box ticked, or with the switch, and not for an update: the offline delete of
; halvo-capture.exe on the directory that the program itself names. Exit 0: the uninstall goes on.
; Exit 75: it aborts with sentence (2); any other end, or no directory named, with sentence (4),
; before any file or value goes.
!macro HALVO_DELETE_EVERYTHING
  ClearErrors
  ${GetOptions} $CMDLINE "${HALVO_DELETE_SWITCH}" $R4
  ${IfNot} ${Errors}
    StrCpy $DeleteAppDataCheckboxState 1
  ${EndIf}
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    !insertmacro HALVO_DATA_DIR_OF_THE_PROGRAM
    ${If} $R9 == ""
      MessageBox MB_ICONSTOP|MB_OK "${HALVO_SENTENCE_NOT_DELETED}" /SD IDOK
      Abort "${HALVO_SENTENCE_NOT_DELETED}"
    ${EndIf}
    nsExec::Exec '"$INSTDIR\halvo-capture.exe" --delete-all --data-dir "$R9"'
    Pop $R4
    ${If} $R4 == "75"
      MessageBox MB_ICONSTOP|MB_OK "${HALVO_SENTENCE_RUNNING}" /SD IDOK
      Abort "${HALVO_SENTENCE_RUNNING}"
    ${ElseIf} $R4 != "0"
      MessageBox MB_ICONSTOP|MB_OK "${HALVO_SENTENCE_NOT_DELETED}" /SD IDOK
      Abort "${HALVO_SENTENCE_NOT_DELETED}"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro HALVO_STOP "${HALVO_SENTENCE_RUNNING_INSTALL}"
!macroend

; After an install: where none of the values that the template's own section WebView2 reads is
; there, sentence (1), and a silent install writes it into the install directory.
!macro NSIS_HOOK_POSTINSTALL
  ${If} ${RunningX64}
    ReadRegStr $R4 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${Else}
    ReadRegStr $R4 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  ${If} $R4 == ""
    ReadRegStr $R4 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  Delete "$INSTDIR\${HALVO_RUNTIME_FILE}"
  ${If} $R4 == ""
    ${If} ${Silent}
      ClearErrors
      FileOpen $R5 "$INSTDIR\${HALVO_RUNTIME_FILE}" w
      ${IfNot} ${Errors}
        FileWrite $R5 "${HALVO_SENTENCE_RUNTIME}$\r$\n"
        FileClose $R5
      ${EndIf}
    ${Else}
      MessageBox MB_ICONINFORMATION|MB_OK "${HALVO_SENTENCE_RUNTIME}"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro HALVO_STOP "${HALVO_SENTENCE_RUNNING}"
  !insertmacro HALVO_DELETE_EVERYTHING
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Halvo"
  ${EndIf}
  Delete "$INSTDIR\${HALVO_RUNTIME_FILE}"
!macroend

; With the box ticked and not for an update: the template's own step has removed the folder of the
; app's id, which holds the web view's folder EBWebView. Where it still stands, the step runs again
; for at most 20 s, since the web view's processes end a moment after the app; then sentence (5).
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    StrCpy $R6 0
    ${DoWhile} ${FileExists} "$LOCALAPPDATA\${BUNDLEID}\*.*"
      ${If} $R6 >= ${HALVO_WAIT_MS}
        ${Break}
      ${EndIf}
      Sleep 500
      IntOp $R6 $R6 + 500
      RmDir /r "$LOCALAPPDATA\${BUNDLEID}"
    ${Loop}
    ${If} ${FileExists} "$LOCALAPPDATA\${BUNDLEID}\*.*"
      nsis_tauri_utils::StrReplace "${HALVO_SENTENCE_FOLDER_STAYS}" "{folder}" "$LOCALAPPDATA\${BUNDLEID}"
      Pop $R4
      MessageBox MB_ICONEXCLAMATION|MB_OK "$R4" /SD IDOK
    ${EndIf}
  ${EndIf}
!macroend
