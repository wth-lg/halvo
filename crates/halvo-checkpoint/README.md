# halvo-checkpoint

The checkpoint of the sheet executor, without a reader of workbooks (contracts.md A11.3, A6.6, A7.4): the name of the copy that `prepare` of halvo-sheet keeps beside a workbook, the walk of a path inside a folder, the SHA-256 of a file, the lock of a workbook's directory and the marks of live prepared copies in `agents/locks/`, and `remove`, the one call that removes a named checkpoint.

## The data directory of the locks, and the first limit

The locks and the marks live in `agents/locks/` under Halvo's data directory, and two calls exclude each other only while both opened the same data directory. `remove` takes the locks that its caller opened; in this tree no caller of `remove` stands outside this crate's tests, and the supervisor library (halvo-agent-client), which is to call it (U4.3), holds the data directory that the app gives it. The runner (halvo-runner) opens its locks under the directory of its optional argument `--data-dir <dir>`, read by halvo-control's `DataDir::from_flag` (an empty value or a relative path is refused as not absolute, a parent step is refused); without the argument it takes the app's default, `DataDir::for_current_user` (`~/Library/Application Support/Halvo` on macOS, the bundle's `data` folder under the account's LocalAppData on Windows).

**The first limit, until the supervisor passes the directory:** the supervisor does not pass `--data-dir` yet. An app that runs with another data directory (halvo-control writes `--data-dir <dir>` into the daemon's launch arguments when its service names a data directory, service.rs `DATA_DIR_ARG`; no reader of that flag stands in this tree, so whether a release build takes it is open) and a runner that takes the default would each hold a lock of their own for one folder, so a `prepare` of the runner and a `remove` of the supervisor would not exclude each other. The harness of halvo-runner passes `--data-dir` with a directory of each test's own, and `a_file_step_locks_under_the_data_directory_that_the_harness_gives` pins that the runner locks under the directory of the argument. No test runs a file step of the runner without the argument, because such a run would lock under this account's real data directory: `the_data_directory_is_optional_once_and_absolute` pins only that the argument is optional and read once, and the default is taken in `folder()` of the runner's file.rs alone. The product's answer, proposed to the planner: A6.1 gains `--data-dir`, the supervisor passes the directory that the app resolved, the argument becomes required and the default goes.

## The identity of a file

Every check of this crate and of halvo-sheet that says "this is the file that I checked" compares two `FileId`s with `same_file` (`src/identity.rs`). No other source file of either crate reads what names a file on the system; `tests/identity.rs` fails when one does.

- macOS: the device and the inode number of the file.
- Windows: the volume's serial number and the file's 128-bit id, read for an open handle with `GetFileInformationByHandleEx` and the class `FileIdInfo` (the structure `FILE_ID_INFO`), through the bindings `windows-sys` 0.61.2, which the workspace's lock already held. Length and times are not part of the identity.

## The limit

Windows documents the file id as stable only on some file systems. The documentation of `BY_HANDLE_FILE_INFORMATION` says that on FAT the id comes from the first cluster of the directory that holds the entry and the offset of the entry in it, that it can change when a defragmenter moves the entry, and that a rename to a longer name can change it; on NTFS a file keeps its id until it is deleted, and ReFS gives the 128-bit id of `FILE_ID_INFO`. `GetVolumeInformationByHandleW` reports `FILE_SUPPORTS_OPEN_BY_FILE_ID` for a volume whose ids can open a file.

On a volume that does not report that flag (FAT, FAT32 and exFAT among them), `same_file` also compares the length, the creation time and the time of the last write, the old test. The check then refuses rather than takes a file that it cannot tell apart: two files with one id but other times are not the same file, and a checkpoint whose id changed when `remove` moved it aside under its longer removing name is put back and left as `changed`. On such a volume `remove` may therefore leave every checkpoint, and a file that another program put in the checked file's place with the same id, length and times cannot be told from it.

On macOS the inode of FAT and exFAT volumes (msdos and exfat) is made by the system from the place of the entry, and the crate reads no flag for it; this README names it as untested.
