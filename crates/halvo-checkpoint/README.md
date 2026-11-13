# halvo-checkpoint

The checkpoint of the sheet executor, without a reader of workbooks (contracts.md A11.3, A6.6, A7.4): the name of the copy that `prepare` of halvo-sheet keeps beside a workbook, the walk of a path inside a folder, the SHA-256 of a file, the lock of a workbook's directory and the marks of live prepared copies in `agents/locks/`, and `remove`, the one call that removes a named checkpoint.

## The identity of a file

Every check of this crate and of halvo-sheet that says "this is the file that I checked" compares two `FileId`s with `same_file` (`src/identity.rs`). No other source file of either crate reads what names a file on the system; `tests/identity.rs` fails when one does.

- macOS: the device and the inode number of the file.
- Windows: the volume's serial number and the file's 128-bit id, read for an open handle with `GetFileInformationByHandleEx` and the class `FileIdInfo` (the structure `FILE_ID_INFO`), through the bindings `windows-sys` 0.61.2, which the workspace's lock already held. Length and times are not part of the identity.

## The limit

Windows documents the file id as stable only on some file systems. The documentation of `BY_HANDLE_FILE_INFORMATION` says that on FAT the id comes from the first cluster of the directory that holds the entry and the offset of the entry in it, that it can change when a defragmenter moves the entry, and that a rename to a longer name can change it; on NTFS a file keeps its id until it is deleted, and ReFS gives the 128-bit id of `FILE_ID_INFO`. `GetVolumeInformationByHandleW` reports `FILE_SUPPORTS_OPEN_BY_FILE_ID` for a volume whose ids can open a file.

On a volume that does not report that flag (FAT, FAT32 and exFAT among them), `same_file` also compares the length, the creation time and the time of the last write, the old test. The check then refuses rather than takes a file that it cannot tell apart: two files with one id but other times are not the same file, and a checkpoint whose id changed when `remove` moved it aside under its longer removing name is put back and left as `changed`. On such a volume `remove` may therefore leave every checkpoint, and a file that another program put in the checked file's place with the same id, length and times cannot be told from it.

On macOS the inode of FAT and exFAT volumes (msdos and exfat) is made by the system from the place of the entry, and the crate reads no flag for it; this README names it as untested.
