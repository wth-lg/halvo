(function () {
  "use strict";
  var Halvo = window.Halvo || (window.Halvo = {});

  Halvo.words = {
    CONNECTING: "Connecting to Halvo's capture service",
    NOT_RUNNING: "Capture service not running",
    HELD_UNREACHABLE: "Halvo's capture service is not answering. Capture may still be running. Halvo keeps trying.",
    STATUS_REFUSED: "Halvo's capture service reported a state that Halvo cannot rely on. Capture may be running.",
    UNSUPPORTED: "Capture is not available on Windows in this version.",
    CAPTURE_RUNNING: "Halvo is capturing",
    CAPTURE_PARTIAL: "Halvo is capturing, but not every part of capture runs",
    CAPTURE_STARTING: "Halvo's capture is starting",
    CAPTURE_PAUSING: "Capture is stopping",
    CAPTURE_PAUSED: "Halvo is paused",
    CAPTURE_NO_INDICATOR: "Halvo is not capturing while it cannot show that it runs",
    CAPTURE_AWAITING_RESUME: "Capture is off until you press Resume",
    CAPTURE_FAULTED: "Halvo is not capturing",
    CAPTURE_UNKNOWN: "Halvo cannot tell whether capture runs",
    FAULT_STORE: "Its store is not open.",
    FAULT_DRAIN: "It cannot save what it records.",
    FAULT_NO_SOURCE: "No part of capture could start.",
    FAULT_PERMISSION: "A permission that capture needs is off.",
    PAUSE_NOT_REACHED: "Capture was not paused. Try again, or quit Halvo to stop capture.",
    RESUME_NOT_REACHED: "Capture did not start. Try again.",
    PAUSE: "Pause",
    RESUME: "Resume",
    PAUSING: "Pausing",
    STARTING: "Starting",
    DELETING: "Deleting",
    READ_FAILED: "Halvo could not read the state of its capture service.",
    SOURCE_FAILED: "has stopped",
    SOURCE_WALKBACK: "The steps under Permissions turn it back on.",
    NOT_SAVING: "Not saving:",
    CAUSE_NO_SPACE: "the disk is full.",
    CAUSE_IO: "a read or a write failed.",
    CAUSE_DB: "the database failed.",
    CAUSE_REJECTED: "the store refused what was recorded.",
    EXPIRY_FAILING: "Old capture is not being deleted:",
    EXPIRY_SINCE: "It last worked at",
    KEEPS_WINDOW: "Halvo keeps the last {days} days of your work on this computer, encrypted. Anything older is deleted automatically.",
    KEEPS_UNKNOWN: "What Halvo records shows here once its capture service answers.",
    ON_DISK: "On disk",
    RECORDED: "Recorded",
    OF_A_CAP: "of a cap of",
    CAP_UNKNOWN: "unknown",
    GB_IS: "GB here means 1,073,741,824 bytes (2 to the power of 30).",
    INVENTORY_UNAVAILABLE: "The list of what is stored is not available right now.",
    NOTHING_STORED: "Nothing stored yet.",
    DAY_UTC: "Day (UTC)",
    DAY_STARTS: "A day here is a UTC day: it starts at {hour} on this computer's clock.",
    SCREEN_ACTIVITY: "Screen activity",
    STORE_OPENING: "Halvo is opening its store.",
    STORE_DELETING: "Halvo is deleting everything.",
    STORE_SEALED: "Halvo's store cannot be opened any more. Delete everything removes it.",
    STORE_SEALED_CUSTODY: "Halvo's store was made with another kind of key protection than this build uses. Delete everything removes it.",
    STORE_UNAVAILABLE: "Halvo's store cannot be opened right now.",
    STORE_RETRY: "Halvo tries again in {seconds} s.",
    STORE_ROOT_DENIED: "This account cannot read the key of Halvo's store. Delete everything removes it.",
    STORE_NO_RELEASE_CUSTODY: "This build can keep up an existing store but does not capture. Delete everything removes it.",
    STORE_OLDER: "Halvo's store was made by an older version of Halvo that this version cannot open. Delete everything removes it.",
    STORE_NEWER: "Halvo's store was made by a newer version of Halvo. Update Halvo to open it, or delete everything.",
    STORE_UNKNOWN: "Halvo's store is in a state that this version does not know:",
    STORE_LOCKED: "Another copy of Halvo's recorder is already using your data. Quit the other copy or restart this computer. Nothing was lost or changed.",
    OBSTRUCTED_HEADING: "Halvo is not recording. Something is in the way of its data folder.",
    OBSTRUCTED_TEXT: "Something that is not Halvo's stands where Halvo keeps what it records, for example a shortcut. Halvo did not look behind it and changed nothing, so it cannot tell you whether your recordings are safe. Put Halvo's own folder back in its place (Show folder), or ask the person who set up this computer. Halvo looks again every minute and starts recording by itself when the folder is back.",
    DELETE_QUESTION: "Delete everything that Halvo has recorded on this computer? This cannot be undone. Copies that Halvo made next to your own files stay.",
    DELETE_ABOUT: "Destroys the keys to every stored capture so nothing can be read back. This cannot be undone.",
    DELETE_DONE: "Deleted. Nothing that Halvo recorded can be read again, and nothing is left.",
    DELETE_NOT_FOLLOWED: "Halvo destroyed its keys, so nothing that was recorded can be read again. Something that was not Halvo's stood in its data folder, or the folder was moved while Halvo was running. Halvo removed nothing in that place. Files may be left there. They cannot be read, and you can remove them.",
    DELETE_ROOT_LEFT: "Halvo could not remove one key file. Remove it with this command:",
    DELETE_ROOT_UNKNOWN: "Halvo cannot tell whether a key file is left.",
    DELETE_FILES_LEFT: "Some files could not be removed: {count}. They cannot be read without the keys.",
    DELETE_DATABASE_LEFT: "A database file could not be removed. It cannot be read without the keys.",
    CLEANUP_NAMES_ACCOUNT: "This line names a folder on this computer and your account name. Take it out before you send it to someone, if you prefer.",
    ALREADY_DELETED: "That store was already deleted. Anything captured since then is still stored.",
    DELETE_REFUSED: "Halvo did not delete anything this time. What is stored now does not match what this page showed you when you pressed, or Halvo could not read it for a moment. An earlier delete may already have removed your data, and what is stored now may be new. Look at the page, then press Delete everything again if you still want to.",
    LOST_STOPPED: "Access was turned off for: {names}. That part of capture has stopped until you turn it back on below.",
    LOST_RUNNING: "Access was turned off for: {names}. That part of capture keeps running until capture restarts; turn it back on below to keep it.",
    CAP_ENTER: "Enter the cap in GB.",
    CAP_RANGE: "The cap must be from {min} GB to {max} GB.",
    CAP_NOT_KNOWN_YET: "The stored size is not known yet. Try again in a moment.",
    CAP_LOWER: "A cap of {gb} GB deletes about {amount} of the oldest capture now. This cannot be undone.",
    CAP_EVICTING: "Halvo is removing the oldest capture to fit the new cap.",
    ERROR_BUSY: "Halvo's capture service is busy with another change. Try again in a moment.",
    ERROR_DEADLINE: "Halvo's capture service did not answer within {seconds} s.",
    ERROR_INVALID: "Halvo's capture service refused the request.",
    ERROR_BAD_ID: "The page held no valid store id. Press Delete everything again.",
    ERROR_UPDATE: "Halvo's capture service is of another version. Update Halvo.",
    ERROR_STORE: "Halvo's store is not open.",
    ERROR_STATE_FILE: "Halvo could not save its settings.",
    ERROR_INTERNAL: "Something went wrong inside Halvo. Try again.",
    ERROR_PEER: "Halvo did not trust the program that answered as its capture service.",
    ERROR_BROKEN: "Halvo could not read the answer of its capture service.",
    ERROR_CHANNEL: "The connection to Halvo's capture service failed.",
    ERROR_PANE: "Halvo could not open System Settings.",
    ERROR_NOT_ANSWERING: "Halvo's capture service did not take the request.",
    ERROR_OTHER: "The request failed.",
    DISMISS: "Dismiss",
    STORAGE_CAP: "Storage cap",
    SAVE: "Save",
    LOWER_CAP: "Lower the cap",
    KEEP_CAP: "Keep the current cap",
    KEEP_AT_MOST: "Keep at most",
    DELETE_EVERYTHING: "Delete everything",
    DELETE_YES: "Yes, delete everything",
    CANCEL: "Cancel",
    KEEPS_HEADING: "What Halvo keeps",
    GRANT_ON: "On",
    GRANT_LOST: "Lost",
    GRANT_NOT_ASKED: "Not asked yet",
    GRANT_OFF: "Off",
    OPEN_SETTINGS: "Open System Settings",
    ASK_NOW: "Ask now",
    PERMISSIONS_HEADING: "Permissions",
    WALKBACK_INTRO: "To get capture working again:",
    CAPTURE_HEADING: "Capture",
    SOURCE_FOREGROUND: "The app in front",
    SOURCE_INPUT: "Typing and the pointer",
    SOURCE_POINTER: "The pointer",
    KIND_TYPING: "Typing",
    KIND_MOUSE: "Mouse",
    KIND_APP_IN_FRONT: "App in front",
    KIND_WINDOW_TITLES: "Window titles",
    KIND_SYSTEM_AUDIO: "System audio",
    KIND_MICROPHONE: "Microphone",
    KIND_CAMERA: "Camera",
    STORED_HEADING: "Stored on this computer",
    COLUMN_WHAT: "What",
    COLUMN_EVENTS: "Events",
    COLUMN_SIZE: "Size",
    UNIT_B: " B",
    UNIT_KB: "KB",
    UNIT_MB: "MB",
    UNIT_GB: "GB",
    UNIT_TB: "TB",
  };

  Halvo.errorWords = function (e) {
    var w = Halvo.words;
    var code = e && typeof e === "object" ? e.error : null;
    switch (code) {
      case "busy":
        return w.ERROR_BUSY;
      case "deadline":
        return w.ERROR_DEADLINE.replace("{seconds}", String(Number(e.seconds) || 0));
      case "invalid_argument":
        return w.ERROR_INVALID;
      case "cap_out_of_range":
        return w.CAP_RANGE.replace("{min}", String(Math.round(e.min_bytes / 1073741824))).replace("{max}", String(Math.round(e.max_bytes / 1073741824)));
      case "bad_store_id":
        return w.ERROR_BAD_ID;
      case "update_the_app":
        return w.ERROR_UPDATE;
      case "store_unavailable":
        return w.ERROR_STORE;
      case "state_file":
        return w.ERROR_STATE_FILE;
      case "internal":
        return w.ERROR_INTERNAL;
      case "peer_rejected":
        return w.ERROR_PEER;
      case "broken_reply":
        return w.ERROR_BROKEN;
      case "channel_failed":
        return w.ERROR_CHANNEL;
      case "pane_not_opened":
        return w.ERROR_PANE;
      case "not_answering":
        return w.ERROR_NOT_ANSWERING;
      default:
        return w.ERROR_OTHER;
    }
  };
})();
