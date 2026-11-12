reading trial=dialog machine=mac system=macos default_attr=AXDefaultButton cancel_attr=AXCancelButton
reading trial=dialog machine=mac system=windows default_attr=STATE_SYSTEM_DEFAULT cancel_attr=IDCANCEL
reading trial=dialog machine=mac form=plugin system=macos default=reject cancel=none close=none file=rfd-0.17.2/src/backend/macos/utils/user_alert.rs line=71
reading trial=dialog machine=mac form=plugin system=windows default=reject cancel=allow close=allow file=tauri-plugin-dialog-2.8.4/src/desktop.rs line=238
reading trial=dialog machine=mac form=system system=macos default=reject cancel=none close=none file=tools/r4-trials/driver/src/native.rs line=72
reading trial=dialog machine=mac form=system system=windows default=reject cancel=reject close=reject file=tools/r4-trials/driver/src/native.rs line=113
cell trial=dialog machine=hosted-mac form=plugin text=same default=reject cancel=none close=none allow_control=allow reject_control=reject driven=yes
cell trial=dialog machine=hosted-mac form=system text=same default=reject cancel=none close=none allow_control=allow reject_control=reject driven=yes
cell trial=dialog machine=hosted-win form=plugin text=same default=reject cancel=allow close=allow allow_control=allow reject_control=reject driven=yes
cell trial=dialog machine=hosted-win form=system text=same default=reject cancel=reject close=reject allow_control=allow reject_control=reject driven=yes
verdict dialog hosted_mac=plugin hosted_win=system driven_mac=yes driven_win=yes
