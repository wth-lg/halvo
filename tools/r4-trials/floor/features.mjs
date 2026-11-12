// usage: node tools/r4-trials/floor/features.mjs DATA_JSON [--verify LIST]
// DATA_JSON is data.json of @mdn/browser-compat-data 8.1.8. Without --verify it prints the lines
// that U0.3 adds to docs/r4/page-features.txt; with --verify it checks that every line of LIST
// whose comment cites a page of that data gives the Safari version and date that the data gives.
// Exit: 0 when every row and line holds, 1 when one misses, 2 on a refusal.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { compareVersions, FLOOR, parseList } from './list.mjs';

const member = (m) => `(?:\\?\\.|\\.)\\s*${m}\\b|\\[\\s*['"]${m}['"]\\s*\\]`;
const fixed = (s, m) => `\\b${s}\\s*(?:\\?\\.|\\.)\\s*${m}\\b|\\b${s}\\s*\\[\\s*['"]${m}['"]\\s*\\]`;
const event = (e) => `['"]${e}['"]|\\bon${e}\\b`;
const word = (w) => `\\b${w}\\b`;
const any = (...parts) => parts.join('|');

/** The allow lines of the floor (rulings RD-27 (b), RD-30 (6)): each web API that a view file
 * of this run calls and that the list had no line for, with its data path and expression. */
export const ALLOW = [
  ['click_event', 'api.Element.click_event', "'onclick' in window"],
  ['input_event', 'api.Element.input_event', "'oninput' in window"],
  ['scroll_top', 'api.Element.scrollTop', "'scrollTop' in Element.prototype"],
  ['focus_method', 'api.HTMLElement.focus', "typeof HTMLElement.prototype.focus === 'function'"],
  ['node_contains', 'api.Node.contains', "typeof Node.prototype.contains === 'function'"],
  ['element_remove', 'api.Element.remove', "typeof Element.prototype.remove === 'function'"],
  ['create_element', 'api.Document.createElement', "typeof document.createElement === 'function'"],
  ['create_text_node', 'api.Document.createTextNode', "typeof document.createTextNode === 'function'"],
  ['append_child', 'api.Node.appendChild', "typeof Node.prototype.appendChild === 'function'"],
  ['insert_before', 'api.Node.insertBefore', "typeof Node.prototype.insertBefore === 'function'"],
  ['remove_child', 'api.Node.removeChild', "typeof Node.prototype.removeChild === 'function'"],
  ['first_child', 'api.Node.firstChild', "'firstChild' in Node.prototype"],
  ['next_sibling', 'api.Node.nextSibling', "'nextSibling' in Node.prototype"],
  ['set_attribute', 'api.Element.setAttribute', "typeof Element.prototype.setAttribute === 'function'"],
  ['get_attribute', 'api.Element.getAttribute', "typeof Element.prototype.getAttribute === 'function'"],
  ['has_attribute', 'api.Element.hasAttribute', "typeof Element.prototype.hasAttribute === 'function'"],
  ['query_selector_all', 'api.Element.querySelectorAll', "typeof Element.prototype.querySelectorAll === 'function'"],
  ['class_list', 'api.Element.classList', "'classList' in Element.prototype && typeof DOMTokenList.prototype.add === 'function'"],
  ['class_name', 'api.Element.className', "'className' in Element.prototype"],
  ['tag_name', 'api.Element.tagName', "'tagName' in Element.prototype"],
  ['active_element', 'api.Document.activeElement', "'activeElement' in Document.prototype"],
  ['add_event_listener', 'api.EventTarget.addEventListener', "typeof EventTarget.prototype.addEventListener === 'function'"],
  ['prevent_default', 'api.Event.preventDefault', "typeof Event.prototype.preventDefault === 'function'"],
  ['event_target', 'api.Event.target', "'target' in Event.prototype"],
  ['event_current_target', 'api.Event.currentTarget', "'currentTarget' in Event.prototype"],
  ['dialog_open', 'api.HTMLDialogElement.open', "'open' in HTMLDialogElement.prototype"],
  ['dialog_close', 'api.HTMLDialogElement.close', "typeof HTMLDialogElement.prototype.close === 'function'"],
  ['input_checked', 'api.HTMLInputElement.checked', "'checked' in HTMLInputElement.prototype"],
  ['input_value', 'api.HTMLInputElement.value', "'value' in HTMLInputElement.prototype"],
  ['style_element', 'api.HTMLStyleElement', "typeof HTMLStyleElement === 'function'"],
  ['json_stringify', 'javascript.builtins.JSON.stringify', "typeof JSON.stringify === 'function'"],
  ['array_is_array', 'javascript.builtins.Array.isArray', "typeof Array.isArray === 'function'"],
  ['array_methods', 'javascript.builtins.Array.reduce', "['forEach', 'map', 'filter', 'some', 'every', 'reduce', 'indexOf', 'join', 'push'].every(function (m) { return typeof Array.prototype[m] === 'function'; })"],
  ['string_from_char_code', 'javascript.builtins.String.fromCharCode', "typeof String.fromCharCode === 'function'"],
  ['string_trim', 'javascript.builtins.String.trim', "typeof String.prototype.trim === 'function'"],
  ['promise_methods', 'javascript.builtins.Promise.resolve', "typeof Promise.resolve === 'function' && typeof Promise.prototype.then === 'function' && typeof Promise.prototype.catch === 'function'"],
  ['map_and_set_methods', 'javascript.builtins.Map.values', "['get', 'set', 'has', 'delete', 'keys', 'values'].every(function (m) { return typeof Map.prototype[m] === 'function'; }) && ['add', 'has', 'delete'].every(function (m) { return typeof Set.prototype[m] === 'function'; })"],
];

/** The deny lines: features whose first Safari version is above the floor and that a view could
 * use, each with its data path and the source of a pattern that finds a use of it. */
export const DENY = [
  ['object_group_by', 'javascript.builtins.Object.groupBy', fixed('Object', 'groupBy')],
  ['map_group_by', 'javascript.builtins.Map.groupBy', fixed('Map', 'groupBy')],
  ['promise_with_resolvers', 'javascript.builtins.Promise.withResolvers', fixed('Promise', 'withResolvers')],
  ['abort_signal_any', 'api.AbortSignal.any_static', fixed('AbortSignal', 'any')],
  ['check_visibility', 'api.Element.checkVisibility', member('checkVisibility')],
  ['aria_description', 'api.Element.ariaDescription', member('ariaDescription')],
  ['error_capture_stack_trace', 'javascript.builtins.Error.captureStackTrace', fixed('Error', 'captureStackTrace')],
  ['array_buffer_transfer', 'javascript.builtins.ArrayBuffer.transfer', any(member('transfer'), member('transferToFixedLength'))],
  ['start_view_transition', 'api.Document.startViewTransition', member('startViewTransition')],
  ['get_html', 'api.Element.getHTML', member('getHTML')],
  ['aria_braille_label', 'api.Element.ariaBrailleLabel', member('ariaBrailleLabel')],
  ['aria_braille_role_description', 'api.Element.ariaBrailleRoleDescription', member('ariaBrailleRoleDescription')],
  ['writing_suggestions', 'api.HTMLElement.writingSuggestions', any(member('writingSuggestions'), word('writingsuggestions'))],
  ['pointer_coalesced_events', 'api.PointerEvent.getCoalescedEvents', member('getCoalescedEvents')],
  ['pointer_predicted_events', 'api.PointerEvent.getPredictedEvents', member('getPredictedEvents')],
  ['pointer_altitude_angle', 'api.PointerEvent.altitudeAngle', member('altitudeAngle')],
  ['pointer_azimuth_angle', 'api.PointerEvent.azimuthAngle', member('azimuthAngle')],
  ['auxclick_event', 'api.Element.auxclick_event', event('auxclick')],
  ['pagereveal_event', 'api.Window.pagereveal_event', event('pagereveal')],
  ['float16_array', 'javascript.builtins.Float16Array', word('Float16Array')],
  ['math_f16round', 'javascript.builtins.Math.f16round', fixed('Math', 'f16round')],
  ['data_view_float16', 'javascript.builtins.DataView.getFloat16', any(member('getFloat16'), member('setFloat16'))],
  ['promise_try', 'javascript.builtins.Promise.try', fixed('Promise', 'try')],
  ['regexp_escape', 'javascript.builtins.RegExp.escape', fixed('RegExp', 'escape')],
  ['uint8_array_base64', 'javascript.builtins.Uint8Array.toBase64', any(member('toBase64'), member('fromBase64'), member('setFromBase64'))],
  ['uint8_array_hex', 'javascript.builtins.Uint8Array.toHex', any(member('toHex'), member('fromHex'), member('setFromHex'))],
  ['dialog_request_close', 'api.HTMLDialogElement.requestClose', member('requestClose')],
  ['focus_visible_option', 'api.HTMLElement.focus.options_focusVisible_parameter', word('focusVisible')],
  ['toggle_popover_force', 'api.HTMLElement.togglePopover.options_force_parameter', `(?:\\?\\.|\\.)\\s*togglePopover\\s*\\(\\s*[^\\s)]`],
  ['iterator_helpers', 'javascript.builtins.Iterator.map', any(word('Iterator'), `\\b(?:values|keys|entries)\\s*\\(\\s*\\)\\s*(?:\\?\\.|\\.)\\s*(?:map|filter|take|drop|flatMap|reduce|toArray|forEach|some|every|find)\\b`, `(?:\\?\\.|\\.)\\s*(?:toArray|take|drop)\\s*\\(`)],
  ['json_raw', 'javascript.builtins.JSON.rawJSON', any(fixed('JSON', 'rawJSON'), fixed('JSON', 'isRawJSON'))],
  ['dialog_toggle_event', 'api.HTMLElement.toggle_event.dialog_elements', any(event('toggle'), event('beforetoggle'))],
  ['show_popover_source', 'api.HTMLElement.showPopover.options_source_parameter', `(?:\\?\\.|\\.)\\s*showPopover\\s*\\(\\s*[^\\s)]`],
  ['custom_element_registry', 'api.Element.customElementRegistry', member('customElementRegistry')],
  ['command_invokers', 'api.HTMLButtonElement.commandForElement', any(member('commandForElement'), word('commandfor'), word('CommandEvent'), event('command'), 'request-close')],
  ['scrollend_event', 'api.Element.scrollend_event', event('scrollend')],
  ['caret_position_from_point', 'api.Document.caretPositionFromPoint', member('caretPositionFromPoint')],
  ['aria_col_index_text', 'api.Element.ariaColIndexText', member('ariaColIndexText')],
  ['aria_row_index_text', 'api.Element.ariaRowIndexText', member('ariaRowIndexText')],
  ['map_get_or_insert', 'javascript.builtins.Map.getOrInsert', any(member('getOrInsert'), member('getOrInsertComputed'))],
  ['math_sum_precise', 'javascript.builtins.Math.sumPrecise', fixed('Math', 'sumPrecise')],
  ['window_navigation', 'api.Window.navigation', `\\bnavigation\\s*(?:\\?\\.|\\.)\\s*(?:navigate|back|forward|reload|traverseTo|entries|currentEntry|updateCurrentEntry|addEventListener|canGoBack|canGoForward|transition|activation)\\b`],
  ['current_css_zoom', 'api.Element.currentCSSZoom', member('currentCSSZoom')],
  ['aria_notify', 'api.Element.ariaNotify', member('ariaNotify')],
];

/** The Safari version, its date and the page of the data at `dotted`, or a problem. */
export function lookup(data, dotted) {
  let node = data;
  for (const key of dotted.split('.')) node = node?.[key];
  const compat = node?.__compat;
  if (!compat) return { problem: `${dotted} is not in the data` };
  const raw = compat.support.safari;
  const support = Array.isArray(raw) ? raw[0] : raw;
  const version = support?.version_added;
  if (typeof version !== 'string' || !/^[0-9]+(?:\.[0-9]+)*$/.test(version) || support.flags || support.prefix || support.alternative_name || support.partial_implementation) {
    return { problem: `${dotted} has no plain Safari version` };
  }
  const release = data.browsers.safari.releases[version];
  if (!release?.release_date) return { problem: `${dotted}: Safari ${version} has no date in the data` };
  const page = compat.mdn_url ?? `https://developer.mozilla.org/ (${dotted})`;
  return { version, date: release.release_date, page };
}

/** The lines of U0.3 for the list from `data`, or the problems of the rows. */
export function lines(data) {
  const out = [];
  const problems = [];
  const emit = (kind, [name, dotted, cell]) => {
    const found = lookup(data, dotted);
    if (found.problem) {
      problems.push(found.problem);
      return;
    }
    const above = compareVersions(found.version, FLOOR) > 0;
    if (kind === 'allow' && above) problems.push(`${name}: Safari ${found.version} is above the floor`);
    if (kind === 'deny' && !above) problems.push(`${name}: Safari ${found.version} is not above the floor`);
    out.push(`# ${found.page}, Safari ${found.version} of ${found.date}`);
    out.push([kind, name, found.version, cell].join('\t'));
  };
  out.push('# U0.3 wrote the allow lines below for every web API that a view file calls and that had no line (rulings RD-27 (b), RD-30 (6)), and the deny lines after them');
  out.push(`# for the features that reached Safari after the floor and that a view could use; each read on 2026-11-12 from @mdn/browser-compat-data ${data.__meta.version} of ${data.__meta.timestamp.slice(0, 10)}.`);
  for (const row of ALLOW) emit('allow', row);
  for (const row of DENY) emit('deny', row);
  return { out, problems };
}

function verify(data, listText) {
  const problems = [];
  const text = listText.split('\n');
  for (const f of parseList(listText).features) {
    const m = /^# https:\/\/developer\.mozilla\.org\/(?:docs\/\S+|\(([a-z.A-Z_0-9-]+)\)), Safari (\S+) of (\S+)$/.exec(text[f.line - 2] ?? '');
    if (!m) continue;
    const row = [...ALLOW, ...DENY].find((r) => r[0] === f.name);
    if (!row) continue;
    const found = lookup(data, row[1]);
    if (found.problem) problems.push(found.problem);
    else if (found.version !== f.version || found.date !== m[3]) problems.push(`${f.name}: the list gives ${f.version} of ${m[3]}, the data ${found.version} of ${found.date}`);
  }
  return problems;
}

function main(argv) {
  const [file, flag, listFile] = argv;
  if (!file || (flag !== undefined && (flag !== '--verify' || !listFile)) || argv.length > 3) {
    process.stderr.write('REFUSED usage: node tools/r4-trials/floor/features.mjs DATA_JSON [--verify LIST]\n');
    return 2;
  }
  let data;
  try {
    data = JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    process.stderr.write('REFUSED usage: DATA_JSON cannot be read as JSON\n');
    return 2;
  }
  const { out, problems } = lines(data);
  if (flag === '--verify') problems.push(...verify(data, fs.readFileSync(listFile, 'utf8')));
  else if (problems.length === 0) process.stdout.write(out.join('\n') + '\n');
  for (const p of problems) process.stdout.write(`miss ${p}\n`);
  if (flag === '--verify') process.stdout.write(`verify rows=${ALLOW.length + DENY.length} problems=${problems.length}\n`);
  return problems.length === 0 ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
