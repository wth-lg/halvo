use halvo_coord::ports::{Chosen, Files, PortError};
use tauri::{AppHandle, Runtime};

/// The system's own chooser of ONE file (r4 U3.22b), Halvo's own code beside the confirm dialog's
/// (`super::System`), never a plugin's. On a Mac it is NSOpenPanel, run modally on the main
/// thread, as the confirm dialog's alert is. A current macOS may show the panel in its own panel
/// service, a process outside Halvo's, and the capture keeps no name of the panel's list all the
/// same: (1) every source of the capture on a Mac gives its record to the application in front,
/// by the system's own answer (halvo-capture macos/foreground.rs `read_frontmost`; a key and a
/// click take the same read, macos/input.rs), and none reads an event's target process or the
/// window under the pointer; (2) while a person works in the chooser that application is Halvo,
/// whose id the floor's list names, so every record of it is dropped: no app-in-front record, no
/// title, no key, no click (floor/exclude.rs); (3) a record of screen activity holds no picture
/// and no text, and its switch is off by default (macos/screen.rs, floor/record.rs,
/// floor/mod.rs), also while the chooser stands open behind another program. (4) What no file
/// shows: whether macOS ever names the panel service itself as the application in front; if it
/// did, the capture would keep an app-in-front record with the service's id, the title of its
/// focused window, key classes and clicks, as for any program's open panel. On Windows it is
/// IFileOpenDialog, shown on a thread of its own in
/// a single-threaded apartment: at the first event of the dialog whose dialog has a window
/// (r4 U3.22e; an event before the window decides nothing), that window gets the display
/// affinity WDA_EXCLUDEFROMCAPTURE, as the confirm dialog's window gets
/// it at TDN_CREATED, so that the system keeps it out of screenshots, screen shares and screen
/// recordings that are made through Windows' own capture features, Halvo's own screen record
/// among them; where the system refuses that call, the event sink closes the dialog at once
/// through IFileDialog::Close from inside that event and the show answers `Chosen::Closed`, as it
/// does where no event of the dialog had a window. Each takes ONE
/// file and no folder (`macos::choices`, `windows::choices`), in the folder given, and answers
/// its full path, or `Chosen::Closed` for a close and for every call that failed. On another
/// system no chooser is shown and the answer is `Chosen::Closed`. Named limits: no run opens the
/// Mac's chooser; the Windows chooser is opened by a hosted run alone (r4 U3.22e), whose one case
/// reads that it stands with the affinity and its owner and closes it by its window's close
/// message, and whose second case reads that a refused affinity closes it at once through the
/// sink's own IFileDialog::Close, and no run opens it with a person's input; no run has read
/// which application macOS names as in front while the panel
/// is up; and the other windows that the system's chooser shows for itself on Windows (the name
/// box's list of suggestions, tooltips) get no affinity. On Windows the dialog is shown with the
/// app's window main as its owner (r4 U3.22e), read at each show, since that window is destroyed
/// and made again at its next show (`crate::window::CLOSE`), and with no owner where the app has
/// no such window: while it is open the system keeps it above that window and holds that window
/// disabled, so a click on Halvo's window brings the chooser to the front and does nothing else.
pub struct SystemChooser<R: Runtime> {
    #[cfg(any(target_os = "macos", windows))]
    app: AppHandle<R>,
    #[cfg(not(any(target_os = "macos", windows)))]
    runtime: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> SystemChooser<R> {
    /// The chooser of the app `app`.
    pub fn new(app: AppHandle<R>) -> SystemChooser<R> {
        #[cfg(any(target_os = "macos", windows))]
        return SystemChooser { app };
        #[cfg(not(any(target_os = "macos", windows)))]
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
        return Ok(windows::choose(
            windows::owner(&self.app),
            folder,
            windows::Watch::PRODUCT,
        ));
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = folder;
            Ok(Chosen::Closed)
        }
    }

    /// The system's chooser reads no names of a workbook: `Product` reads them by the runner's
    /// program (r4 U3.22c) and never asks the chooser.
    fn sheets(&self, _: &str) -> Result<Vec<String>, PortError> {
        Err(PortError::NoService)
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
    use std::ffi::c_void;
    use tauri::{AppHandle, Manager, Runtime};
    use windows::core::{implement, ComObject, Interface, Ref, Result, HSTRING};
    use windows::Win32::Foundation::{E_ABORT, HWND};
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

    /// The window main of `app`, which owns the chooser, read at the show, as a number that
    /// crosses to the chooser's thread; none where the app has no such window or its handle
    /// cannot be read.
    pub fn owner<R: Runtime>(app: &AppHandle<R>) -> Option<isize> {
        app.get_webview_window(crate::handler::MAIN)
            .and_then(|window| window.hwnd().ok())
            .map(|window| window.0 as isize)
    }

    /// What the code hands to the dialog: its options, the folder in which it opens, the
    /// display affinity of its window, and the window that owns it.
    pub struct Choices {
        pub options: FILEOPENDIALOGOPTIONS,
        pub folder: Option<String>,
        pub affinity: WINDOW_DISPLAY_AFFINITY,
        pub owner: Option<HWND>,
    }

    /// Over the options `start` that the dialog has: ONE file of the file system that must exist
    /// and no folder, never many; in `folder`, or where the system opens the dialog when none is
    /// given; its window kept out of the capture by WDA_EXCLUDEFROMCAPTURE, and owned by `owner`,
    /// by no window where none is given.
    pub fn choices(
        start: FILEOPENDIALOGOPTIONS,
        folder: Option<&str>,
        owner: Option<HWND>,
    ) -> Choices {
        let wanted = FOS_FILEMUSTEXIST.0 | FOS_FORCEFILESYSTEM.0 | FOS_PATHMUSTEXIST.0;
        let unwanted = FOS_ALLOWMULTISELECT.0 | FOS_PICKFOLDERS.0;
        Choices {
            options: FILEOPENDIALOGOPTIONS((start.0 | wanted) & !unwanted),
            folder: folder.map(str::to_owned),
            affinity: WDA_EXCLUDEFROMCAPTURE,
            owner,
        }
    }

    /// How one show sets the affinity and what it tells on the dialog's thread (r4 U3.22e): the
    /// product's is `Watch::PRODUCT`, which sets the affinity through the system and tells
    /// nothing; the hosted cases pass their own.
    #[derive(Clone, Copy)]
    pub struct Watch {
        /// Sets the display affinity of the dialog's window, and answers whether it took.
        pub protect: fn(HWND, WINDOW_DISPLAY_AFFINITY) -> bool,
        /// At the event that decided: the dialog, its window, and, where the affinity did not
        /// take, what the close from inside that event answered.
        pub decided: fn(&IFileDialog, HWND, Option<&Result<()>>),
        /// What the dialog's Show answered.
        pub shown: fn(&Result<()>),
    }

    impl Watch {
        /// The product's.
        pub const PRODUCT: Watch = Watch {
            protect,
            decided: told_nothing,
            shown: shown_nothing,
        };
    }

    /// Sets `affinity` on `window` through the system, and answers whether it took.
    fn protect(window: HWND, affinity: WINDOW_DISPLAY_AFFINITY) -> bool {
        // SAFETY: the window of the dialog whose event calls this, which lives while it shows.
        unsafe { SetWindowDisplayAffinity(window, affinity) }.is_ok()
    }

    fn told_nothing(_: &IFileDialog, _: HWND, _: Option<&Result<()>>) {}

    fn shown_nothing(_: &Result<()>) {}

    /// The events of one show (r4 U3.22e): at the first event whose dialog has a window it sets
    /// the affinity on that window by `watch.protect`, then tells `watch.decided`; where that
    /// call did not take, it first closes the dialog through IFileDialog::Close from inside that
    /// event. An event whose dialog has no window yet decides nothing. `set` holds whether the
    /// call took, none before that event.
    #[implement(IFileDialogEvents)]
    struct Guard {
        affinity: WINDOW_DISPLAY_AFFINITY,
        set: Cell<Option<bool>>,
        watch: Watch,
    }

    impl Guard {
        fn first(&self, dialog: Ref<IFileDialog>) {
            if self.set.get().is_some() {
                return;
            }
            let Ok(dialog) = dialog.ok() else {
                return;
            };
            // SAFETY: the window of the dialog that sent this event, which lives while it shows.
            let Some(window) = dialog
                .cast::<IOleWindow>()
                .and_then(|w| unsafe { w.GetWindow() })
                .ok()
                .filter(|window| !window.is_invalid())
            else {
                return;
            };
            let protected = (self.watch.protect)(window, self.affinity);
            self.set.set(Some(protected));
            if protected {
                (self.watch.decided)(dialog, window, None);
            } else {
                // SAFETY: a call on the dialog that sent this event, from inside the event, while
                // it shows.
                let closed = unsafe { dialog.Close(E_ABORT) };
                (self.watch.decided)(dialog, window, Some(&closed));
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

        fn OnSelectionChange(&self, dialog: Ref<IFileDialog>) -> Result<()> {
            self.first(dialog);
            Ok(())
        }

        fn OnShareViolation(
            &self,
            _: Ref<IFileDialog>,
            _: Ref<IShellItem>,
        ) -> Result<FDE_SHAREVIOLATION_RESPONSE> {
            Ok(FDESVR_DEFAULT)
        }

        fn OnTypeChange(&self, dialog: Ref<IFileDialog>) -> Result<()> {
            self.first(dialog);
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

    /// Shows the dialog on a thread of its own, owned by the window `owner` where one is given,
    /// with `watch` (the product's `Watch::PRODUCT`), and waits for its answer.
    pub fn choose(owner: Option<isize>, folder: Option<&str>, watch: Watch) -> Chosen {
        let folder = folder.map(str::to_owned);
        std::thread::Builder::new()
            .name("halvo-chooser".into())
            .spawn(move || show(owner, folder.as_deref(), watch))
            .ok()
            .and_then(|shown| shown.join().ok())
            .unwrap_or(Chosen::Closed)
    }

    fn show(owner: Option<isize>, folder: Option<&str>, watch: Watch) -> Chosen {
        // SAFETY: the apartment of this new thread, left below once the dialog has gone.
        let joined =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        if joined.is_err() {
            return Chosen::Closed;
        }
        let owner = owner.map(|owner| HWND(owner as *mut c_void));
        let chosen = pick(owner, folder, watch).unwrap_or(Chosen::Closed);
        // SAFETY: the one CoInitializeEx of this thread that succeeded above.
        unsafe { CoUninitialize() };
        chosen
    }

    fn pick(owner: Option<HWND>, folder: Option<&str>, watch: Watch) -> Result<Chosen> {
        // SAFETY: the system's file open dialog, made in this thread's apartment.
        let dialog: IFileOpenDialog =
            unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }?;
        // SAFETY: calls on the dialog made above, on the thread that made it.
        let made = choices(unsafe { dialog.GetOptions() }?, folder, owner);
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
            watch,
        });
        let events: IFileDialogEvents = guard.to_interface();
        // SAFETY: the events of this show, removed again before the dialog is dropped.
        let cookie = unsafe { dialog.Advise(&events) }?;
        // SAFETY: a call on the dialog made above, on its thread; the owner is a window handle,
        // which the system checks, of the app's window main or of the hosted case's own window,
        // read at this show.
        let shown = unsafe { dialog.Show(made.owner) };
        (watch.shown)(&shown);
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
    /// keeps the names of the panel's list out of what Halvo stores is the capture's own rule,
    /// which `SystemChooser` gives in (1) to (4) and which this test does not read: every source
    /// gives its record to the application in front, Halvo is that application while a person
    /// works in the chooser and the floor drops every record of it, and a record of screen
    /// activity holds no picture and no text; no file shows whether macOS ever names the panel
    /// service as the application in front.
    #[cfg(target_os = "macos")]
    #[test]
    fn u322b_the_mac_chooser_takes_one_file_and_no_folder_in_the_given_folder() {
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
            let made = choices(FILEOPENDIALOGOPTIONS(start), Some("C:\\work"), None);
            assert_eq!(made.options.0 & wanted, wanted, "{start:#x}");
            assert_eq!(made.options.0 & unwanted, 0, "{start:#x}");
            assert_eq!(made.folder.as_deref(), Some("C:\\work"));
            assert_eq!(made.affinity, WDA_EXCLUDEFROMCAPTURE);
        }
        assert_eq!(choices(FILEOPENDIALOGOPTIONS(0), None, None).folder, None);
    }

    /// r4 U3.22e sentence 2, the Windows code with no window: what the code hands to
    /// IFileOpenDialog holds the owner that it was given, the window with which `pick` shows the
    /// dialog, and no owner where none is given.
    #[cfg(windows)]
    #[test]
    fn u322e_the_windows_chooser_hands_the_dialog_the_owner_that_it_was_given() {
        use super::windows::choices;
        use ::windows::Win32::Foundation::HWND;
        use ::windows::Win32::UI::Shell::FILEOPENDIALOGOPTIONS;
        let owner = HWND(0x0322_e0e0 as *mut std::ffi::c_void);
        let made = choices(FILEOPENDIALOGOPTIONS(0), Some("C:\\work"), Some(owner));
        assert_eq!(made.owner, Some(owner));
        assert_eq!(choices(FILEOPENDIALOGOPTIONS(0), None, None).owner, None);
    }

    /// r4 U3.22e sentences 1 and 2, on a hosted runner of GitHub Actions alone, with no keyboard
    /// and no pointer input: the product's own code shows IFileOpenDialog on a thread of its own,
    /// owned by a window of the case's own in the app's place; within a bounded wait an event of
    /// the dialog has a window that took the display affinity, the window stands visible, reads
    /// WDA_EXCLUDEFROMCAPTURE by the system's own GetWindowDisplayAffinity, has the case's window
    /// as its owner by GetWindow, and has not closed by itself 500 ms later; then the case ends
    /// the chooser as a person's close box does, by the close message WM_CLOSE posted to the
    /// dialog's window, and the show answers `Chosen::Closed`. Every wait has its bound and ends
    /// red with its sentence and with what each call answered, and the case joins no thread that
    /// a stuck dialog holds, so it cannot hang. It writes ONE line to the standard error, past the
    /// capture of the test harness, that starts with its name: on a hosted runner the
    /// milliseconds from the show to the answer closed, and on every other machine the line of
    /// the confirm dialog's hosted cases, after which it passes at once.
    #[cfg(windows)]
    #[test]
    fn u322e_on_a_hosted_runner_the_windows_chooser_opens_out_of_the_capture_and_stays_until_its_window_is_closed(
    ) {
        hosted::case(hosted::OPENING, hosted::opening);
    }

    /// r4 U3.22e, the product's own close, on a hosted runner of GitHub Actions alone, with no
    /// keyboard and no pointer input: where the system refuses the display affinity, the
    /// product's event sink closes the chooser through IFileDialog::Close from inside its own
    /// event. The case hands the product's code a call of the affinity that answers refused. It
    /// waits up to 10 s from the show, the bound of the case above, for that refusal at the
    /// chooser's first event with a window, and up to 5 s from the refusal for the show's answer
    /// (r4 U3.22f); it reads what the close from inside that event answered, what Show answered,
    /// and that the show answers `Chosen::Closed`. Every wait has its bound, so it ends within
    /// the two together, 15 s from the show and one poll of 10 ms. It writes its line as the case
    /// above does, with the milliseconds from the show to the refusal and from the refusal to the
    /// answer.
    #[cfg(windows)]
    #[test]
    fn u322e_on_a_hosted_runner_the_windows_chooser_closes_at_once_through_its_own_interface_where_the_affinity_is_refused(
    ) {
        hosted::case(hosted::REFUSED, hosted::refused);
    }

    #[cfg(windows)]
    mod hosted {
        use super::super::windows::{choose, Watch};
        use ::windows::core::{w, Result as WinResult};
        use ::windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use ::windows::Win32::UI::Shell::IFileDialog;
        use ::windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, GetWindow, GetWindowDisplayAffinity,
            IsWindow, IsWindowVisible, PeekMessageW, PostMessageW, TranslateMessage, GW_OWNER, MSG,
            PM_REMOVE, WDA_EXCLUDEFROMCAPTURE, WINDOW_DISPLAY_AFFINITY, WINDOW_EX_STYLE, WM_CLOSE,
            WS_POPUP,
        };
        use halvo_coord::ports::Chosen;
        use std::ffi::c_void;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::mpsc::{self, Receiver, Sender};
        use std::sync::{Arc, Mutex, PoisonError};
        use std::time::{Duration, Instant};

        const WINDOW_BOUND: Duration = Duration::from_secs(10);
        const STAYS: Duration = Duration::from_millis(500);
        const CLOSE_BOUND: Duration = Duration::from_secs(5);
        const POLL: Duration = Duration::from_millis(10);

        /// The names of the two cases, which start each line that they write.
        pub const OPENING: &str = "present::chooser::tests::u322e_on_a_hosted_runner_the_windows_chooser_opens_out_of_the_capture_and_stays_until_its_window_is_closed";
        pub const REFUSED: &str = "present::chooser::tests::u322e_on_a_hosted_runner_the_windows_chooser_closes_at_once_through_its_own_interface_where_the_affinity_is_refused";

        /// Held by each case for its whole show, so that the two cases, which the harness runs on
        /// threads side by side, never show a chooser at the same time; each keeps its readings
        /// in statics of its own.
        static ALONE: Mutex<()> = Mutex::new(());

        /// The opening case's: where its `decided` sends the window that took the affinity, what
        /// the close from inside the event answered where the affinity did not take, and what
        /// Show answered.
        static OPENING_WINDOW: Mutex<Option<Sender<isize>>> = Mutex::new(None);
        static OPENING_REFUSED: Mutex<Option<String>> = Mutex::new(None);
        static OPENING_SHOWN: Mutex<Option<String>> = Mutex::new(None);
        const OPENING_WATCH: Watch = Watch {
            decided: opening_decided,
            shown: opening_shown,
            ..Watch::PRODUCT
        };

        /// The refused case's: how often the affinity was refused and when first, what the close
        /// from inside the event answered, and what Show answered.
        static REFUSALS: AtomicUsize = AtomicUsize::new(0);
        static REFUSED_AT: Mutex<Option<Instant>> = Mutex::new(None);
        static REFUSED_CLOSE: Mutex<Option<String>> = Mutex::new(None);
        static REFUSED_SHOWN: Mutex<Option<String>> = Mutex::new(None);
        const REFUSED_WATCH: Watch = Watch {
            protect: refuse,
            decided: refused_decided,
            shown: refused_shown,
        };

        fn keep<T>(slot: &Mutex<Option<T>>, value: Option<T>) {
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = value;
        }

        fn read<T: Clone>(slot: &Mutex<Option<T>>) -> Option<T> {
            slot.lock().unwrap_or_else(PoisonError::into_inner).clone()
        }

        /// The answer of a call as the system names it: S_OK, or its HRESULT.
        fn word(answer: &WinResult<()>) -> String {
            match answer {
                Ok(()) => "S_OK".to_owned(),
                Err(e) => format!("{:#010x}", e.code().0),
            }
        }

        fn opening_decided(_: &IFileDialog, window: HWND, closed: Option<&WinResult<()>>) {
            if let Some(closed) = closed {
                keep(&OPENING_REFUSED, Some(word(closed)));
                return;
            }
            if let Some(tx) = OPENING_WINDOW
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
            {
                let _ = tx.send(window.0 as isize);
            }
        }

        fn opening_shown(shown: &WinResult<()>) {
            keep(&OPENING_SHOWN, Some(word(shown)));
        }

        fn refuse(_: HWND, _: WINDOW_DISPLAY_AFFINITY) -> bool {
            if REFUSALS.fetch_add(1, Ordering::SeqCst) == 0 {
                keep(&REFUSED_AT, Some(Instant::now()));
            }
            false
        }

        fn refused_decided(_: &IFileDialog, _: HWND, closed: Option<&WinResult<()>>) {
            if let Some(closed) = closed {
                keep(&REFUSED_CLOSE, Some(word(closed)));
            }
        }

        fn refused_shown(shown: &WinResult<()>) {
            keep(&REFUSED_SHOWN, Some(word(shown)));
        }

        /// Whether this is a hosted runner of GitHub Actions, where the cases run, as the
        /// confirm dialog's hosted cases know one: `GITHUB_ACTIONS` is `true` and
        /// `RUNNER_ENVIRONMENT` is `github-hosted`.
        fn hosted() -> bool {
            std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
                && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
        }

        /// Writes `text` behind the case's name `name` as one line to the standard error itself,
        /// which the test harness does not capture, so that a passing run shows it too.
        fn line(name: &str, text: &str) {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "{name}: {text}");
        }

        /// One case: off a hosted runner its line and a pass; on one, `run` alone among the
        /// cases, which writes the case's line, and a red with every failure.
        pub fn case(name: &str, run: fn(&str) -> Vec<String>) {
            if !hosted() {
                line(name, "hosted alone: not run on this machine");
                return;
            }
            let failures = {
                let _alone = ALONE.lock().unwrap_or_else(PoisonError::into_inner);
                run(name)
            };
            assert!(
                failures.is_empty(),
                "the system's chooser on this hosted runner: {}",
                failures.join("; ")
            );
        }

        fn visible(window: HWND) -> bool {
            // SAFETY: reads of a window of this process, or of one that has gone, which read false.
            unsafe { IsWindow(Some(window)).as_bool() && IsWindowVisible(window).as_bool() }
        }

        fn affinity(window: HWND) -> Option<u32> {
            let mut affinity = 0u32;
            // SAFETY: `affinity` is a live local that the call writes.
            unsafe { GetWindowDisplayAffinity(window, &mut affinity) }
                .ok()
                .map(|()| affinity)
        }

        fn ms(since: Instant) -> u128 {
            since.elapsed().as_millis()
        }

        /// A window of the case's own in the app's place: a hidden top-level window of the
        /// system's class STATIC, made on a thread of its own that hands its messages on, as the
        /// app's main thread does for the window main, until `stop`; where the show never
        /// answers, that thread and the window stay until the test process ends.
        struct Owner {
            window: isize,
            stop: Arc<AtomicBool>,
        }

        fn owner() -> Option<Owner> {
            let stop = Arc::new(AtomicBool::new(false));
            let ends = stop.clone();
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("u322e-owner".into())
                .spawn(move || {
                    // SAFETY: a hidden top-level window of the system's class STATIC, made by this
                    // thread; the class and the title are static strings.
                    let made = unsafe {
                        CreateWindowExW(
                            WINDOW_EX_STYLE(0),
                            w!("STATIC"),
                            w!("u322e owner"),
                            WS_POPUP,
                            0,
                            0,
                            0,
                            0,
                            None,
                            None,
                            None,
                            None,
                        )
                    };
                    let window = made.ok();
                    let _ = tx.send(window.map(|window| window.0 as isize));
                    let Some(window) = window else {
                        return;
                    };
                    let mut msg = MSG::default();
                    while !ends.load(Ordering::SeqCst) {
                        // SAFETY: the messages of this thread, read into a live local.
                        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                            // SAFETY: the message that the call above read.
                            unsafe {
                                let _ = TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                        }
                        std::thread::sleep(POLL);
                    }
                    // SAFETY: the window that this thread made above.
                    let _ = unsafe { DestroyWindow(window) };
                })
                .ok()?;
            rx.recv_timeout(WINDOW_BOUND)
                .ok()
                .flatten()
                .map(|window| Owner { window, stop })
        }

        /// The answer of the show, kept once it has come.
        struct Answer {
            rx: Receiver<Chosen>,
            got: Option<Chosen>,
        }

        impl Answer {
            /// The answer, where it has come within `bound`.
            fn within(&mut self, bound: Duration) -> Option<Chosen> {
                if self.got.is_none() {
                    self.got = self.rx.recv_timeout(bound).ok();
                }
                self.got.clone()
            }
        }

        /// Shows the chooser through the product's code on a thread of the case's own, owned by
        /// `owner`, with `watch`.
        fn show(owner: isize, watch: Watch) -> std::io::Result<Answer> {
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("u322e-chooser".into())
                .spawn(move || {
                    let _ = tx.send(choose(Some(owner), None, watch));
                })?;
            Ok(Answer { rx, got: None })
        }

        /// The end of a case that showed the chooser, after its waits: the owner's thread ends
        /// where the show answered, and else stays, with the chooser, until the test process
        /// ends; then the case's one line, with what Show and the show answered.
        fn end(
            name: &str,
            failures: &[String],
            total: u128,
            mut reads: Vec<String>,
            answer: &mut Answer,
            owner: &Owner,
            shown: &Mutex<Option<String>>,
        ) {
            let got = answer.within(Duration::ZERO);
            if got.is_some() {
                owner.stop.store(true, Ordering::SeqCst);
            }
            reads.push(format!(
                "Show answered {}",
                read(shown).unwrap_or_else(|| "nothing".to_owned())
            ));
            reads.push(format!(
                "the show answered {}",
                got.map_or_else(|| "nothing".to_owned(), |got| format!("{got:?}"))
            ));
            let reads = reads.join(", ");
            line(
                name,
                &if failures.is_empty() {
                    format!("{total} ms from the show to the answer closed ({reads})")
                } else {
                    format!(
                        "red {total} ms after the show, with {} failures ({reads})",
                        failures.len()
                    )
                },
            );
        }

        /// The opening case: what failed, each in its sentence, after its one line.
        pub fn opening(name: &str) -> Vec<String> {
            let mut failures = Vec::new();
            let (window_tx, windows) = mpsc::channel();
            keep(&OPENING_WINDOW, Some(window_tx));
            keep(&OPENING_REFUSED, None);
            keep(&OPENING_SHOWN, None);
            let Some(owner) = owner() else {
                failures
                    .push("the case could not make a window of its own in the app's place".into());
                line(name, "red: no window of the case's own in the app's place");
                return failures;
            };
            let start = Instant::now();
            match show(owner.window, OPENING_WATCH) {
                Ok(mut answer) => {
                    let reads =
                        opening_reads(&mut failures, &windows, &mut answer, owner.window, start);
                    let total = ms(start);
                    // Where the show has not answered yet, one more wait of `CLOSE_BOUND`.
                    answer.within(CLOSE_BOUND);
                    end(
                        name,
                        &failures,
                        total,
                        reads,
                        &mut answer,
                        &owner,
                        &OPENING_SHOWN,
                    );
                }
                Err(e) => {
                    failures.push(format!("no thread for the show: {e}"));
                    line(name, "red: no thread for the show");
                }
            }
            failures
        }

        /// The reads of the opening case, in order; each miss goes to `failures` and ends them.
        /// It answers what it read, for the case's line.
        fn opening_reads(
            failures: &mut Vec<String>,
            windows: &Receiver<isize>,
            answer: &mut Answer,
            owner: isize,
            start: Instant,
        ) -> Vec<String> {
            let mut reads = Vec::new();
            let window = loop {
                if let Ok(window) = windows.try_recv() {
                    break HWND(window as *mut c_void);
                }
                if let Some(answer) = answer.within(Duration::ZERO) {
                    let refused = read(&OPENING_REFUSED);
                    reads.push(format!(
                        "the affinity refused at an event with a window: {}",
                        refused.as_deref().map_or("no", |closed| closed)
                    ));
                    failures.push(format!("the chooser closed by itself {} ms after the show began, with the answer {answer:?}, before any event of it had a window that took the display affinity", ms(start)));
                    return reads;
                }
                if start.elapsed() > WINDOW_BOUND {
                    failures.push(format!("no event of the chooser had a window that took the display affinity within {} s of the show", WINDOW_BOUND.as_secs()));
                    return reads;
                }
                std::thread::sleep(POLL);
            };
            reads.push(format!("an event with a window at {} ms", ms(start)));
            while !visible(window) && start.elapsed() <= WINDOW_BOUND {
                std::thread::sleep(POLL);
            }
            if !visible(window) {
                failures.push(format!(
                    "the chooser's window did not stand visible within {} s of the show",
                    WINDOW_BOUND.as_secs()
                ));
                return reads;
            }
            reads.push(format!("visible at {} ms", ms(start)));
            let read = affinity(window);
            reads.push(format!("display affinity {read:?}"));
            if read != Some(WDA_EXCLUDEFROMCAPTURE.0) {
                failures.push(format!(
                    "the chooser's window read the display affinity {read:?}, not {}",
                    WDA_EXCLUDEFROMCAPTURE.0
                ));
            }
            // SAFETY: a read of a window of this process, or of one that has gone, which fails.
            let owned = unsafe { GetWindow(window, GW_OWNER) }
                .ok()
                .map(|owned| owned.0 as isize);
            reads.push(format!("owner {owned:?} (the case's window {owner})"));
            if owned != Some(owner) {
                failures.push(format!(
                    "the chooser's window has the owner {owned:?}, not the case's window {owner} in the app's place"
                ));
            }
            let stood = Instant::now();
            while stood.elapsed() < STAYS {
                if let Some(answer) = answer.within(Duration::ZERO) {
                    failures.push(format!(
                        "the chooser closed by itself {} ms after its window stood, with the answer {answer:?}",
                        ms(stood)
                    ));
                    return reads;
                }
                std::thread::sleep(POLL);
            }
            if !visible(window) {
                failures.push(format!(
                    "the chooser's window was gone or hidden {} ms after it stood",
                    STAYS.as_millis()
                ));
                return reads;
            }
            reads.push(format!("open {} ms later", STAYS.as_millis()));
            let closing = Instant::now();
            // SAFETY: a message posted to a window of this process, or to one that has gone,
            // whose post fails; the dialog's own loop takes it on the dialog's thread.
            let posted = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            reads.push(format!("WM_CLOSE posted: {}", word(&posted)));
            match answer.within(CLOSE_BOUND) {
                Some(Chosen::Closed) => {
                    reads.push(format!("closed {} ms after the close message", ms(closing)));
                }
                Some(other) => failures.push(format!(
                    "the chooser ended by its close message answered {other:?}, not closed"
                )),
                None => {
                    reads.push(format!(
                        "its window {} {} s after the close message",
                        if visible(window) {
                            "stands"
                        } else {
                            "is gone or hidden"
                        },
                        CLOSE_BOUND.as_secs()
                    ));
                    failures.push(format!(
                        "the chooser did not answer within {} s of the close message posted to its window; it stays on the runner's screen until the test process ends",
                        CLOSE_BOUND.as_secs()
                    ));
                }
            }
            reads
        }

        /// The refused case: what failed, each in its sentence, after its one line.
        pub fn refused(name: &str) -> Vec<String> {
            let mut failures = Vec::new();
            REFUSALS.store(0, Ordering::SeqCst);
            keep(&REFUSED_AT, None);
            keep(&REFUSED_CLOSE, None);
            keep(&REFUSED_SHOWN, None);
            let Some(owner) = owner() else {
                failures
                    .push("the case could not make a window of its own in the app's place".into());
                line(name, "red: no window of the case's own in the app's place");
                return failures;
            };
            let start = Instant::now();
            let mut answer = match show(owner.window, REFUSED_WATCH) {
                Ok(answer) => answer,
                Err(e) => {
                    failures.push(format!("no thread for the show: {e}"));
                    line(name, "red: no thread for the show");
                    return failures;
                }
            };
            let reads = refused_reads(&mut failures, &mut answer, start);
            let total = ms(start);
            end(
                name,
                &failures,
                total,
                reads,
                &mut answer,
                &owner,
                &REFUSED_SHOWN,
            );
            failures
        }

        /// The reads of the refused case, in order; each miss goes to `failures`. It waits for the
        /// refusal at the chooser's first event with a window up to `WINDOW_BOUND` from the show,
        /// and takes an answer of the show that comes before it; behind a refusal it waits for
        /// the answer up to `CLOSE_BOUND` from that refusal. It answers what it read, for the
        /// case's line.
        fn refused_reads(
            failures: &mut Vec<String>,
            answer: &mut Answer,
            start: Instant,
        ) -> Vec<String> {
            // The answer first: `refuse` keeps the refusal before the show can answer, so a
            // refusal read behind an answer is never missed.
            let refused_at = loop {
                let answered = answer.within(Duration::ZERO).is_some();
                if let Some(at) = read(&REFUSED_AT) {
                    break Some(at);
                }
                if answered || start.elapsed() > WINDOW_BOUND {
                    break None;
                }
                std::thread::sleep(POLL);
            };
            let waited = ms(start);
            let (got, after) = match refused_at {
                Some(at) => {
                    let left = (at + CLOSE_BOUND).saturating_duration_since(Instant::now());
                    let got = answer.within(left);
                    (got, Some(ms(at)))
                }
                None => (answer.within(Duration::ZERO), None),
            };
            let mut reads = vec![format!(
                "the affinity refused {} times{}",
                REFUSALS.load(Ordering::SeqCst),
                refused_at.map_or_else(String::new, |at| format!(
                    ", first {} ms from the show",
                    at.duration_since(start).as_millis()
                ))
            )];
            if let (Some(after), Some(_)) = (after, &got) {
                reads.push(format!("the answer {after} ms from the refusal"));
            }
            let close = read(&REFUSED_CLOSE);
            reads.push(format!(
                "the close from inside the event answered {}",
                close.as_deref().unwrap_or("nothing")
            ));
            match (refused_at, got) {
                (None, Some(chosen)) => failures.push(format!(
                    "the chooser closed by itself {waited} ms after the show began, with the answer {chosen:?}, before any event of it had a window, so its sink never decided"
                )),
                (None, None) => failures.push(format!(
                    "no event of the chooser had a window within {} s of the show, so its sink never decided; the case leaves the chooser on the runner's screen",
                    WINDOW_BOUND.as_secs()
                )),
                (Some(_), got) => {
                    if close.is_none() {
                        failures
                            .push("the sink did not close the chooser from inside its event".into());
                    }
                    match got {
                        Some(Chosen::Closed) => {}
                        Some(other) => failures.push(format!(
                            "the chooser whose affinity was refused answered {other:?}, not closed"
                        )),
                        None => failures.push(format!(
                            "the chooser whose affinity was refused did not answer within {} s of the refusal; the case leaves it on the runner's screen",
                            CLOSE_BOUND.as_secs()
                        )),
                    }
                }
            }
            reads
        }
    }
}
