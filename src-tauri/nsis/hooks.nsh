; Uninstalling Recall removes what it stored: the search index, settings and
; log in %LOCALAPPDATA%\Recall (core::data_dir, the same folder the program is
; installed to by default) and the chat sign-ins in WebView2's profile,
; %LOCALAPPDATA%\app.recall.desktop. The person's own files are never touched.
;
; An update runs the old uninstaller with /UPDATE ($UpdateMode = 1) and must
; keep everything, so nothing here runs then.

!define MUI_UNCONFIRMPAGE_TEXT_TOP "Recall will be removed from this computer, along with its search index and chat sign-ins. Your own files are not touched."

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    SetShellVarContext current
    ; the sign-in windows' browser processes can take a moment to let go of their files
    Sleep 1000
    RMDir /r "$LOCALAPPDATA\Recall"
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}
!macroend
