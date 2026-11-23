use halvo_coord::ports::{Chosen, Files, PortError};
use tauri::{AppHandle, Runtime};

/// The system's own chooser of ONE file (r4 U3.22b), Halvo's own code beside the confirm dialog's
/// (`super::System`), never a plugin's. On a Mac it is NSOpenPanel, run modally on the main
/// thread of the app's own process, as the confirm dialog's alert is: Halvo's bundle sets no
/// entitlements, so no sandbox hands the panel to a service of the system, and the panel is a
/// window of the app's own process, which the capture's screen source leaves out with the app,
/// as it leaves the alert out. On Windows it is IFileOpenDialog, shown on a thread of its own in
/// a single-threaded apartment: at the first event that the dialog sends, the window of the
/// dialog gets the display affinity WDA_EXCLUDEFROMCAPTURE, as the confirm dialog's window gets
/// it at TDN_CREATED, so that the system keeps it out of screenshots, screen shares and screen
/// recordings that are made through Windows' own capture features, Halvo's own screen record
/// among them; where the system refuses that call, the dialog closes at once and the show
/// answers `Chosen::Closed`, as it does where the dialog never sent that event. Each takes ONE
/// file and no folder (`macos::choices`, `windows::choices`), in the folder given, and answers
/// its full path, or `Chosen::Closed` for a close and for every call that failed. On another
/// system no chooser is shown and the answer is `Chosen::Closed`. Named limits: no run here
/// opens the chooser, and the other windows that the system's chooser shows for itself on
/// Windows (the name box's list of suggestions, tooltips) get no affinity.
pub struct SystemChooser<R: Runtime> {
    #[cfg(target_os = "macos")]
    app: AppHandle<R>,
    #[cfg(not(target_os = "macos"))]
    runtime: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> SystemChooser<R> {
    /// The chooser of the app `app`.
    pub fn new(app: AppHandle<R>) -> SystemChooser<R> {
        #[cfg(target_os = "macos")]
        return SystemChooser { app };
        #[cfg(not(target_os = "macos"))]
        {
            drop(app);
            SystemChooser {
                runtime: std::marker::PhantomData,
            }
        }
    }
}

impl<R: Runtime> Files for SystemChooser<R> {
    fn choose(&self, folder: Option<&str>) -> Result<Chosen, PortError> {
        #[cfg(target_os = "macos")]
        return Ok(macos::choose(&self.app, folder));
        #[cfg(windows)]
        return Ok(windows::choose(folder));
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = folder;
            Ok(Chosen::Closed)
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) mod macos {
    use halvo_coord::ports::Chosen;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSString, NSURL};
    use std::sync::mpsc;
    use tauri::{AppHandle, Runtime};

    /// What the code hands to the panel: whether it takes files, folders and more than one
    /// item, whether it offers a new folder, and the folder in which it opens.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Choices {
        pub files: bool,
        pub folders: bool,
        pub many: bool,
        pub new_folders: bool,
        pub folder: Option<String>,
    }

    /// ONE file and no folder, never many and no new folder, in `folder`, or where the system
    /// opens the panel when none is given.
    pub fn choices(folder: Option<&str>) -> Choices {
        Choices {
            files: true,
            folders: false,
            many: false,
            new_folders: false,
            folder: folder.map(str::to_owned),
        }
    }

    /// Shows the panel modally on the main thread and waits for its answer: the full path of the
    /// picked file for the response OK; `Chosen::Closed` for every other response, a panel with
    /// no file URL, and a show that could not be queued or that ended with no answer.
    pub fn choose<R: Runtime>(app: &AppHandle<R>, folder: Option<&str>) -> Chosen {
        let made = choices(folder);
        let (tx, rx) = mpsc::channel();
        let queued = app.run_on_main_thread(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseFiles(made.files);
            panel.setCanChooseDirectories(made.folders);
            panel.setAllowsMultipleSelection(made.many);
            panel.setCanCreateDirectories(made.new_folders);
            if let Some(folder) = &made.folder {
                let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(folder), true);
                panel.setDirectoryURL(Some(&url));
            }
            let response = panel.runModal();
            let path = (response == NSModalResponseOK)
                .then(|| panel.URLs().firstObject())
                .flatten()
                .and_then(|url| url.path())
                .map(|path| path.to_string());
            let _ = tx.send(path);
        });
        if queued.is_err() {
            return Chosen::Closed;
        }
        match rx.recv() {
            Ok(Some(path)) => Chosen::Path(path),
            _ => Chosen::Closed,
        }
    }
}

#[cfg(windows)]
pub(super) mod windows {
    use halvo_coord::ports::Chosen;
    use std::cell::Cell;
    use windows::core::{implement, ComObject, Interface, Ref, Result, HSTRING};
    use windows::Win32::Foundation::E_ABORT;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IBindCtx,
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::System::Ole::IOleWindow;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileDialog, IFileDialogEvents, IFileDialogEvents_Impl, IFileOpenDialog,
        IShellItem, SHCreateItemFromParsingName, FDEOR_DEFAULT, FDESVR_DEFAULT,
        FDE_OVERWRITE_RESPONSE, FDE_SHAREVIOLATION_RESPONSE, FILEOPENDIALOGOPTIONS,
        FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
        FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WINDOW_DISPLAY_AFFINITY,
    };

    /// What the code hands to the dialog: its options, the folder in which it opens, and the
    /// display affinity of its window.
    pub struct Choices {
        pub options: FILEOPENDIALOGOPTIONS,
        pub folder: Option<String>,
        pub affinity: WINDOW_DISPLAY_AFFINITY,
    }

    /// Over the options `start` that the dialog has: ONE file of the file system that must exist
    /// and no folder, never many; in `folder`, or where the system opens the dialog when none is
    /// given; its window kept out of the capture by WDA_EXCLUDEFROMCAPTURE.
    pub fn choices(start: FILEOPENDIALOGOPTIONS, folder: Option<&str>) -> Choices {
        let wanted = FOS_FILEMUSTEXIST.0 | FOS_FORCEFILESYSTEM.0 | FOS_PATHMUSTEXIST.0;
        let unwanted = FOS_ALLOWMULTISELECT.0 | FOS_PICKFOLDERS.0;
        Choices {
            options: FILEOPENDIALOGOPTIONS((start.0 | wanted) & !unwanted),
            folder: folder.map(str::to_owned),
            affinity: WDA_EXCLUDEFROMCAPTURE,
        }
    }

    /// The events of one show: at the first event of the dialog's folder it sets the affinity on
    /// the dialog's window, and closes the dialog where that call fails; `set` holds whether the
    /// call took, none before the first event.
    #[implement(IFileDialogEvents)]
    struct Guard {
        affinity: WINDOW_DISPLAY_AFFINITY,
        set: Cell<Option<bool>>,
    }

    impl Guard {
        fn first(&self, dialog: Ref<IFileDialog>) {
            if self.set.get().is_some() {
                return;
            }
            let Ok(dialog) = dialog.ok() else {
                self.set.set(Some(false));
                return;
            };
            // SAFETY: the window of the dialog that sent this event, which lives while it shows.
            let protected = dialog
                .cast::<IOleWindow>()
                .and_then(|w| unsafe { w.GetWindow() })
                .is_ok_and(|window| {
                    unsafe { SetWindowDisplayAffinity(window, self.affinity) }.is_ok()
                });
            self.set.set(Some(protected));
            if !protected {
                // SAFETY: a call on the dialog that sent this event, while it shows.
                let _ = unsafe { dialog.Close(E_ABORT) };
            }
        }
    }

    impl IFileDialogEvents_Impl for Guard_Impl {
        fn OnFileOk(&self, _: Ref<IFileDialog>) -> Result<()> {
            Ok(())
        }

        fn OnFolderChanging(&self, dialog: Ref<IFileDialog>, _: Ref<IShellItem>) -> Result<()> {
            self.first(dialog);
            Ok(())
        }

        fn OnFolderChange(&self, dialog: Ref<IFileDialog>) -> Result<()> {
            self.first(dialog);
            Ok(())
        }

        fn OnSelectionChange(&self, _: Ref<IFileDialog>) -> Result<()> {
            Ok(())
        }

        fn OnShareViolation(
            &self,
            _: Ref<IFileDialog>,
            _: Ref<IShellItem>,
        ) -> Result<FDE_SHAREVIOLATION_RESPONSE> {
            Ok(FDESVR_DEFAULT)
        }

        fn OnTypeChange(&self, _: Ref<IFileDialog>) -> Result<()> {
            Ok(())
        }

        fn OnOverwrite(
            &self,
            _: Ref<IFileDialog>,
            _: Ref<IShellItem>,
        ) -> Result<FDE_OVERWRITE_RESPONSE> {
            Ok(FDEOR_DEFAULT)
        }
    }

    /// Shows the dialog on a thread of its own and waits for its answer.
    pub fn choose(folder: Option<&str>) -> Chosen {
        let folder = folder.map(str::to_owned);
        std::thread::Builder::new()
            .name("halvo-chooser".into())
            .spawn(move || show(folder.as_deref()))
            .ok()
            .and_then(|shown| shown.join().ok())
            .unwrap_or(Chosen::Closed)
    }

    fn show(folder: Option<&str>) -> Chosen {
        // SAFETY: the apartment of this new thread, left below once the dialog has gone.
        let joined =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if joined.is_err() {
            return Chosen::Closed;
        }
        let chosen = pick(folder).unwrap_or(Chosen::Closed);
        // SAFETY: the one CoInitializeEx of this thread that succeeded above.
        unsafe { CoUninitialize() };
        chosen
    }

    fn pick(folder: Option<&str>) -> Result<Chosen> {
        // SAFETY: the system's file open dialog, made in this thread's apartment.
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }?;
        // SAFETY: calls on the dialog made above, on the thread that made it.
        let made = choices(unsafe { dialog.GetOptions() }?, folder);
        unsafe { dialog.SetOptions(made.options) }?;
        if let Some(folder) = &made.folder {
            // SAFETY: the item of a path, made in this thread's apartment.
            let item = unsafe {
                SHCreateItemFromParsingName::<_, _, IShellItem>(
                    &HSTRING::from(folder.as_str()),
                    None::<&IBindCtx>,
                )
            };
            if let Ok(item) = item {
                // SAFETY: as above.
                let _ = unsafe { dialog.SetFolder(&item) };
            }
        }
        let guard = ComObject::new(Guard {
            affinity: made.affinity,
            set: Cell::new(None),
        });
        let events: IFileDialogEvents = guard.to_interface();
        // SAFETY: the events of this show, removed again before the dialog is dropped.
        let cookie = unsafe { dialog.Advise(&events) }?;
        let shown = unsafe { dialog.Show(None) };
        let _ = unsafe { dialog.Unadvise(cookie) };
        if shown.is_err() || guard.set.get() != Some(true) {
            return Ok(Chosen::Closed);
        }
        // SAFETY: the item that the person picked, and its name, which the system allocated and
        // which is freed once it is read.
        let item = unsafe { dialog.GetResult() }?;
        let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }?;
        let path = unsafe { name.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(name.0 as *const _)) };
        Ok(path.map_or(Chosen::Closed, Chosen::Path))
    }
}

#[cfg(test)]
mod tests {
    /// r4 U3.22b sentence 1, OF WEIGHT, the Mac's code: the choices that the code hands to
    /// NSOpenPanel, made by a function with no window: ONE file and no folder, never many and no
    /// new folder, in the folder given and where the system opens it when none is given. What
    /// keeps the panel's window out of Halvo's own capture as it keeps the confirm dialog's: the
    /// panel is a window of the app's own process, which the capture's screen source leaves out
    /// with the app; the app's bundle sets no entitlements, so no sandbox hands the panel to a
    /// service of the system, whose window the capture would take.
    #[cfg(target_os = "macos")]
    #[test]
    fn u322b_the_mac_chooser_takes_one_file_and_no_folder_in_the_given_folder_in_the_apps_own_process(
    ) {
        use super::macos::{choices, Choices};
        assert_eq!(
            choices(Some("/Volumes/work")),
            Choices {
                files: true,
                folders: false,
                many: false,
                new_folders: false,
                folder: Some("/Volumes/work".to_owned()),
            }
        );
        assert_eq!(choices(None).folder, None);
        for bundle in [
            include_str!("../../../../src-tauri/tauri.conf.json"),
            include_str!("../../../../src-tauri/tauri.macos.conf.json"),
        ] {
            assert!(!bundle.contains("entitlements"), "{bundle}");
        }
    }

    /// r4 U3.22b sentence 1, OF WEIGHT, the Windows code: the choices that the code hands to
    /// IFileOpenDialog, made by a function with no window: over any options that the dialog
    /// starts with, ONE file of the file system that must exist and no folder, never many, in the
    /// folder given and where the system opens it when none is given; and what keeps its window
    /// out of Halvo's own capture as the confirm dialog's window is kept out: the display
    /// affinity WDA_EXCLUDEFROMCAPTURE.
    #[cfg(windows)]
    #[test]
    fn u322b_the_windows_chooser_takes_one_file_and_no_folder_in_the_given_folder_out_of_the_capture(
    ) {
        use super::windows::choices;
        use ::windows::Win32::UI::Shell::{
            FILEOPENDIALOGOPTIONS, FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
            FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
        };
        use ::windows::Win32::UI::WindowsAndMessaging::WDA_EXCLUDEFROMCAPTURE;
        let wanted = FOS_FILEMUSTEXIST.0 | FOS_FORCEFILESYSTEM.0 | FOS_PATHMUSTEXIST.0;
        let unwanted = FOS_ALLOWMULTISELECT.0 | FOS_PICKFOLDERS.0;
        for start in [0, unwanted, u32::MAX] {
            let made = choices(FILEOPENDIALOGOPTIONS(start), Some("C:\\work"));
            assert_eq!(made.options.0 & wanted, wanted, "{start:#x}");
            assert_eq!(made.options.0 & unwanted, 0, "{start:#x}");
            assert_eq!(made.folder.as_deref(), Some("C:\\work"));
            assert_eq!(made.affinity, WDA_EXCLUDEFROMCAPTURE);
        }
        assert_eq!(choices(FILEOPENDIALOGOPTIONS(0), None).folder, None);
    }
}
