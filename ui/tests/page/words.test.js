"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { APP } = require("../lib/page.js");

const WORDS = {
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
  DELETE_DONE: "Deleted. Nothing that Halvo recorded in its data folder can be read again, and nothing of it is left there.",
  DELETE_KEYS_DESTROYED: "Halvo destroyed its keys, so nothing that was recorded can be read again.",
  DELETE_NOT_FOLLOWED: "Something that was not Halvo's stood in its data folder, or the folder was moved while Halvo was running. Halvo removed nothing in that place. Files may be left there. They cannot be read, and you can remove them.",
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
  CAP_EVICTING: "Halvo removes the oldest capture to fit the new cap. The size above shows what is stored now.",
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
  DELETE_ENTRIES_LEFT: "Halvo could not remove {count} of its own folders or files in its data folder. Something that Halvo recorded may still be in them.",
  DELETE_UNKNOWN_ENTRIES: "Halvo's data folder holds {count} things that Halvo does not know. Halvo did not open or remove them.",
  DELETE_ROOT_UNNAMED: "Halvo could not remove one key file and cannot name it. What was recorded may still be readable on this computer.",
  DELETE_COPIES: "Copies that Halvo made next to your own files stay. A copy or a backup of Halvo's whole data folder that was made before can still be opened, and Halvo cannot reach it. Remove those yourself if you want them gone.",
  DELETE_RESULT_UNREADABLE: "Halvo could not show the result of your delete. Look at what this page shows as stored.",
  DELETE_UNKNOWN_OUTCOME: "Halvo cannot tell whether your data was deleted. Look at what this page shows as stored.",
  DELETE_AT: "At {time}: ",
  DELETE_ON_DAY: "On {date} at {time}: ",
  CAP_ANSWER_LOST: "If the cap was lowered, older capture may have been removed.",
  CAP_NOT_REACHED: "Halvo could not reach its capture service, so the cap was not changed.",
  CAP_NOT_CHANGED: "The cap was not changed.",
  CAP_UNKNOWN_OUTCOME: "Halvo cannot tell whether the cap was changed. Look at the cap shown here once the capture service answers. If it was lowered, older capture may have been removed.",
  DELETE_BUSY: "This press deleted nothing. Halvo's capture service is busy with another change. Press Delete everything again in a moment.",
  DELETE_REFUSED_TWICE: "This is the second time in a row that Halvo could not delete.",
  DELETE_NOTHING_REMOVED: "This press deleted nothing. Look at what this page shows as stored.",
  DELETE_SERVICE_NOT_BACK: "Halvo's capture service shut down during your delete and did not start again in time, so Halvo could not ask it again.",
  DELETE_SERVICE_ENDING: "This press deleted nothing, because Halvo's capture service was shutting down.",
  DELETE_NOT_FOLLOWED_KEY_LEFT: "Something that was not Halvo's stood in its data folder, or the folder was moved while Halvo was running. Halvo removed nothing in that place. Files may be left there, and you can remove them.",
  DELETE_FILES_COUNT: "Some files could not be removed: {count}.",
  DELETE_DATABASE_ALONE: "A database file could not be removed.",
  DELETE_OFFLINE_QUESTION: "Halvo's capture service is not running. Delete everything that Halvo has stored in its data folder, {folder}? This cannot be undone. Copies that Halvo made next to your own files stay.",
  DELETE_OFFER_OFFLINE: "Halvo can stop its capture service and delete without it.",
  DELETE_STOP_AND_DELETE: "Stop capture and delete everything",
  DELETE_STOP_QUESTION: "Stop Halvo's capture service and delete everything that Halvo has stored in its data folder, {folder}? This cannot be undone. Copies that Halvo made next to your own files stay. Halvo starts its capture service again after the delete.",
  DELETE_NO_STORE: "Nothing of what Halvo records was stored in its data folder, {folder}, so there was nothing to delete.",
  DELETE_HELD: "This press deleted nothing, because another program is using Halvo's data folder, {folder}. Quit any other copy of Halvo or restart this computer, then press Delete everything again.",
  DELETE_NOT_USABLE: "This press deleted nothing, because Halvo could not use its data folder, {folder}.",
  DELETE_NOT_STARTED: "This press deleted nothing, because Halvo could not start its delete.",
  DELETE_LOOKED_IN: "The delete ran in Halvo's data folder, {folder}.",
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
  ASK_REQUESTED: "Halvo asked macOS for the {pane} permission. If a message from macOS appears, answer it there. Your choice then shows here.",
  ASK_ALREADY_ON: "The {pane} permission is already on.",
  ASK_UNSUPPORTED: "Halvo's capture service cannot ask macOS for this permission right now. Quit Halvo, open it again, and try once more.",
  LEAVE_OFF: "Leave it off",
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
  NOTICED_HEADING: "What Halvo noticed",
  CATEGORY_UTILITY_BILL_PAYMENT: "Paying a utility bill",
  CATEGORY_TENANT_RENT: "Handling a tenant's rent",
  CATEGORY_LEASE_RENEWAL_OR_CHANGE: "Renewing or changing a lease",
  CATEGORY_TENANT_APPLICATION_REVIEW: "Reviewing a tenant application",
  CATEGORY_EMAIL: "Writing or answering email",
  CATEGORY_SPREADSHEET_FILL: "Filling in a spreadsheet",
  CATEGORY_SHOP_ORDER: "Ordering from a shop",
  CATEGORY_CALENDAR_CLEANUP: "Tidying a calendar",
  CATEGORY_RIDE_ORDER: "Ordering a ride",
  CATEGORY_DAILY_CHART_CHECK: "Checking a chart",
  SEEN_WHERE: "{category} in {app} on {site}",
  SEEN_COUNT: "Seen {count} times.",
  SEEN_COUNT_DAYS: "Seen {count} times, from {first day} to {last day}.",
  APPROVE: "Approve",
  NOT_NOW: "Not now",
  STORED_WORKFLOWS: "Workflows stored on this computer",
  DELETE_WORKFLOW: "Delete",
  DELETE_WORKFLOW_QUESTION: "Delete this workflow? This cannot be undone.",
  DELETE_WORKFLOW_YES: "Yes, delete it",
  NOT_LOOKING: "Halvo is not looking for repeated tasks yet.",
  NOTHING_FOUND: "Halvo has not found a repeated task yet.",
  LOOKS_WHEN: "Halvo looks for repeated tasks while this computer is plugged in and not in use.",
  WORKFLOW_IN_USE: "Halvo did not delete this workflow, because an agent is still using it. Try again later.",
  CAPTURE_HELD: "Capture is off. One part of capture did not stop in time, so Halvo shut its capture service down to make sure that nothing is being read. Nothing is recorded until you turn capture back on.",
  HELD_RESUME: "Press Resume to turn capture back on.",
  HELD_START: "This happened three times in a row, so Halvo did not start its capture service again. Press Start to turn capture back on.",
  START: "Start",
  LOGIN_ITEM_OFF: "Halvo should start by itself when you log in. If it does not, open System Settings, then General, then Login Items, and turn Halvo on.",
};

const OWNER = {
  "I-73 (1)": "HELD_UNREACHABLE",
  "I-73 (2)": "STATUS_REFUSED",
  "I-73 (3) pause": "PAUSE_NOT_REACHED",
  "I-73 (3) resume": "RESUME_NOT_REACHED",
  "I-73 (4)": "DELETE_REFUSED",
  "I-77 (1) heading": "OBSTRUCTED_HEADING",
  "I-77 (1) text": "OBSTRUCTED_TEXT",
  "I-77 (2)": "DELETE_NOT_FOLLOWED",
  "I-77 (2), its first sentence": "DELETE_KEYS_DESTROYED",
  "I-114 (1)": "DELETE_ENTRIES_LEFT",
  "I-114 (2)": "DELETE_UNKNOWN_ENTRIES",
  "I-114 (3)": "DELETE_ROOT_UNNAMED",
  "I-114 (4)": "DELETE_DONE",
  "I-114 (5)": "DELETE_COPIES",
  "I-114 (8)": "DELETE_RESULT_UNREADABLE",
  "I-114 (6)": "DELETE_UNKNOWN_OUTCOME",
  "I-114 (7)": "DELETE_AT",
  "I-114 (19)": "DELETE_ON_DAY",
  "I-114 (18)": "CAP_ANSWER_LOST",
  "I-114 (9)": "CAP_NOT_REACHED",
  "I-114 (10)": "CAP_NOT_CHANGED",
  "I-114 (11)": "CAP_UNKNOWN_OUTCOME",
  "I-114 (12)": "CAP_EVICTING",
  "I-114 (13)": "DELETE_BUSY",
  "I-114 (16)": "DELETE_NOT_FOLLOWED_KEY_LEFT",
  "I-114 (17), files": "DELETE_FILES_COUNT",
  "I-114 (17), database": "DELETE_DATABASE_ALONE",
  "I-77 (3)": "STORE_LOCKED",
  "I-77 (4)": "CLEANUP_NAMES_ACCOUNT",
};

function loaded() {
  const context = {};
  context.window = context;
  vm.createContext(context);
  vm.runInContext(fs.readFileSync(path.join(APP, "words.js"), "utf8"), context);
  return context.Halvo.words;
}

test("each sentence of the page stands once in words.js and equals its literal", () => {
  const words = loaded();
  assert.deepEqual(Object.keys(words), Object.keys(WORDS));
  for (const [name, literal] of Object.entries(WORDS)) assert.equal(words[name], literal, name);
  const values = Object.values(words);
  assert.equal(new Set(values).size, values.length, "no sentence stands twice");
});

test("the owner's proposals of entries I-73 and I-77 are named constants of words.js", () => {
  const words = loaded();
  for (const [entry, name] of Object.entries(OWNER)) assert.ok(typeof words[name] === "string" && words[name].length > 0, entry);
});

const NOT_READ = [
  ["*", "use strict", "the directive of strict mode"],
  ["agents.js", "Your agents", "45-r4-words reads the heading of an R4 view from its line heading: \"...\" in the view file"],
  ["needs.js", "What needs you", "45-r4-words reads the heading of an R4 view from its line heading: \"...\" in the view file"],
  ["agents.js", "Enter", "a key name compared with event.key"],
  ["needs.js", "Enter", "a key name compared with event.key"],
  ["agents.js", "BUTTON", "a tag name compared with tagName"],
  ["needs.js", "BUTTON", "a tag name compared with tagName"],
  ["frame.js", "DOMContentLoaded", "an event name"],
  ["delete.js", "UTC", "the time zone in which toLocaleDateString reads a date that is already shifted to the clock of this computer"],
  ["stored.js", "the inventory reply has no list of rows", "the message of an Error: Halvo.showError shows ERROR_BROKEN for an Error, never its message"],
  ["cap.js", "confirm-row hidden", "a class list"],
  ["delete.js", "confirm-row hidden", "a class list"],
  ["delete.js", "muted hidden", "a class list"],
  ["stored.js", "muted hidden", "a class list"],
  ["permissions.js", "banner hidden", "a class list"],
  ["permissions.js", "muted why", "a class list"],
  ["permissions.js", "muted walkback-intro", "a class list"],
  ["status.js", "row status-row", "a class list"],
];

const READ = /[A-Z]|\p{L} \p{L}/u;

function literalsOf(text) {
  const found = [];
  let i = 0;
  let line = 1;
  while (i < text.length) {
    const c = text[i];
    if (c === "\n") {
      line += 1;
      i += 1;
    } else if (c === "/" && text[i + 1] === "/") {
      while (i < text.length && text[i] !== "\n") i += 1;
    } else if (c === "/" && text[i + 1] === "*") {
      const end = text.indexOf("*/", i + 2);
      assert.notEqual(end, -1, "a block comment ends");
      line += text.slice(i, end).split("\n").length - 1;
      i = end + 2;
    } else if (c === '"' || c === "'" || c === "`") {
      const start = line;
      let value = "";
      i += 1;
      while (text[i] !== c) {
        assert.ok(i < text.length, "a literal ends");
        if (text[i] === "\\") {
          value += text[i] + text[i + 1];
          i += 2;
        } else {
          if (text[i] === "\n") line += 1;
          value += text[i];
          i += 1;
        }
      }
      i += 1;
      found.push({ line: start, value });
    } else {
      i += 1;
    }
  }
  return found;
}

function readFiles() {
  const dir = path.join(APP, "views");
  return fs
    .readdirSync(dir)
    .filter((f) => f.endsWith(".js"))
    .sort()
    .map((f) => path.join(dir, f))
    .concat([path.join(APP, "frame.js")]);
}

test("no file under ui/app/views and not ui/app/frame.js holds a string literal a person reads: one with an upper-case letter, or with a letter, a space and a letter, unless the allow-list holds it", () => {
  const read = [];
  const allowedSeen = new Set();
  for (const file of readFiles()) {
    const name = path.basename(file);
    for (const { line, value } of literalsOf(fs.readFileSync(file, "utf8"))) {
      if (!READ.test(value)) continue;
      const allowed = NOT_READ.findIndex(([where, literal]) => (where === "*" || where === name) && literal === value);
      if (allowed === -1) read.push(name + ":" + line + " " + JSON.stringify(value));
      else allowedSeen.add(allowed);
    }
  }
  const stale = NOT_READ.filter((_, index) => !allowedSeen.has(index)).map(([where, literal]) => where + " " + JSON.stringify(literal));
  assert.deepEqual(stale, [], "the allow-list names a literal that no file holds");
  assert.deepEqual(read, [], read.length + " literals a person reads stand outside words.js");
});
