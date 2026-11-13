The message dialog of tauri-plugin-dialog 2.8.4, which rfd 0.17.2 shows on macOS through the system's alert call, hands the app the second label when that call fails, when the system answers with a cancel and for a response that rfd does not list, and the second label is the allowing one.
So the form fails on macOS under I4.9, and on Windows it fails by its cancel and close controls.

reading trial=dialog machine=mac system=macos default_attr=AXDefaultButton cancel_attr=AXCancelButton
reading trial=dialog machine=mac system=windows default_attr=STATE_SYSTEM_DEFAULT cancel_attr=IDCANCEL
reading trial=dialog machine=mac form=plugin system=macos default=reject cancel=none close=none file=rfd-0.17.2/src/backend/macos/utils/user_alert.rs line=71
reading trial=dialog machine=mac form=plugin system=windows default=reject cancel=allow close=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238
reading trial=dialog machine=mac form=system system=macos default=reject cancel=none close=none file=tools/r4-trials/driver/src/native.rs line=72
reading trial=dialog machine=mac form=system system=windows default=reject cancel=reject close=reject file=tools/r4-trials/driver/src/native.rs line=113
reading trial=dialog machine=mac form=plugin system=macos outcome=failed_call gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238
reading trial=dialog machine=mac form=plugin system=macos outcome=cancel_response gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238
reading trial=dialog machine=mac form=plugin system=macos outcome=unknown_response gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238
reading trial=dialog machine=mac form=plugin system=macos outcome=timeout gives=none file=rfd-0.17.2/src/backend/macos/utils/user_alert.rs line=60
reading trial=dialog machine=mac form=plugin system=windows outcome=failed_call gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238
reading trial=dialog machine=mac form=plugin system=windows outcome=cancel_response gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238
reading trial=dialog machine=mac form=plugin system=windows outcome=unknown_response gives=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238
reading trial=dialog machine=mac form=plugin system=windows outcome=timeout gives=none file=rfd-0.17.2/src/backend/win_cid/message_dialog.rs line=153
reading trial=dialog machine=mac form=system system=macos outcome=failed_call gives=reject file=tools/r4-trials/driver/src/native.rs line=80
reading trial=dialog machine=mac form=system system=macos outcome=cancel_response gives=reject file=tools/r4-trials/driver/src/card.rs line=95
reading trial=dialog machine=mac form=system system=macos outcome=unknown_response gives=reject file=tools/r4-trials/driver/src/card.rs line=95
reading trial=dialog machine=mac form=system system=macos outcome=timeout gives=none file=tools/r4-trials/driver/src/native.rs line=78
reading trial=dialog machine=mac form=system system=windows outcome=failed_call gives=reject file=tools/r4-trials/driver/src/card.rs line=104
reading trial=dialog machine=mac form=system system=windows outcome=cancel_response gives=reject file=tools/r4-trials/driver/src/card.rs line=104
reading trial=dialog machine=mac form=system system=windows outcome=unknown_response gives=reject file=tools/r4-trials/driver/src/card.rs line=104
reading trial=dialog machine=mac form=system system=windows outcome=timeout gives=none file=tools/r4-trials/driver/src/native.rs line=108
cell trial=dialog machine=hosted-mac form=plugin text=same default=reject cancel=none close=none allow_control=allow reject_control=reject driven=yes
cell trial=dialog machine=hosted-mac form=system text=same default=reject cancel=none close=none allow_control=allow reject_control=reject driven=yes
cell trial=dialog machine=hosted-win form=plugin text=same default=reject cancel=allow close=allow allow_control=allow reject_control=reject driven=yes
cell trial=dialog machine=hosted-win form=system text=same default=reject cancel=reject close=reject allow_control=allow reject_control=reject driven=yes
verdict dialog hosted_mac=system hosted_win=system driven_mac=yes driven_win=yes
