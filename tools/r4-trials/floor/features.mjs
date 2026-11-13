// usage: node tools/r4-trials/floor/features.mjs DATA_JSON [--verify LIST]
// DATA_JSON is data.json of @mdn/browser-compat-data 8.1.8. Without --verify it prints the lines
// that U0.3 adds to docs/r4/page-features.txt; with --verify it checks that every line of LIST
// whose comment cites a page of that data gives the Safari version and date that the data gives,
// and that the list ends with the lines that the rows make; it prints how many feature lines it
// read and how many it did not, because they have no row or no comment of the data above them.
// Exit: 0 when every row and line holds, 1 when one misses, 2 on a refusal.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { compareVersions, FLOOR, parseList } from './list.mjs';

const member = (m) => `(?:\\?\\.|\\.)${m}\\b|\\[\\s*['"]${m}['"]\\s*\\]`;
const call = (m) => `(?:\\?\\.|\\.)${m}\\s*\\(`;
const fixed = (s, m) => `\\b${s}\\s*(?:\\?\\.|\\.)\\s*${m}\\b|\\b${s}\\s*(?:\\?\\.)?\\s*\\[\\s*['"]${m}['"]\\s*\\]|\\{[^{}]*\\b${m}\\b[^{}]*\\}\\s*=\\s*${s}\\b`;
const css = (property, js) => `\\b${property}\\b|\\b${js}\\b`;
const event = (e) => `['"]${e}['"]|\\bon${e}\\b`;
const word = (w) => `\\b${w}\\b`;
const any = (...parts) => parts.join('|');
const HELPERS = '(?:map|filter|take|drop|flatMap|reduce|toArray|forEach|some|every|find)\\b';

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
  ['map_and_set_methods', 'javascript.builtins.Map.values', "['get', 'set', 'has', 'delete', 'keys', 'values', 'forEach'].every(function (m) { return typeof Map.prototype[m] === 'function'; }) && ['add', 'has', 'delete', 'forEach'].every(function (m) { return typeof Set.prototype[m] === 'function'; })"],
  ['event_type', 'api.Event.type', "'type' in Event.prototype"],
  ['element_id', 'api.Element.id', "'id' in Element.prototype"],
  ['input_type', 'api.HTMLInputElement.type', "'type' in HTMLInputElement.prototype"],
  ['input_name', 'api.HTMLInputElement.name', "'name' in HTMLInputElement.prototype"],
  ['button_type', 'api.HTMLButtonElement.type', "'type' in HTMLButtonElement.prototype"],
  ['node_list', 'api.NodeList.length', "'length' in NodeList.prototype && typeof NodeList.prototype.item === 'function'"],
  ['string_function', 'javascript.builtins.String.String', "typeof String === 'function' && String(1) === '1'"],
  ['boolean_function', 'javascript.builtins.Boolean.Boolean', "typeof Boolean === 'function' && Boolean(1) === true"],
  ['symbol_iterator', 'javascript.builtins.Symbol.iterator', "typeof Symbol === 'function' && typeof Symbol.iterator === 'symbol' && typeof ''[Symbol.iterator] === 'function' && typeof new Map().values()[Symbol.iterator] === 'function'"],
  ['css_flex', 'css.properties.display.flex', "CSS.supports('display', 'flex') && CSS.supports('flex-wrap', 'wrap') && CSS.supports('align-items', 'center')"],
  ['css_inline_block', 'css.properties.display.inline-block', "CSS.supports('display', 'inline-block')"],
  ['css_border_radius', 'css.properties.border-radius', "CSS.supports('border-radius', '50%')"],
  ['css_box_sizing', 'css.properties.box-sizing', "CSS.supports('box-sizing', 'border-box')"],
  ['css_outline_offset', 'css.properties.outline-offset', "CSS.supports('outline-offset', '2px')"],
  ['css_rgb_alpha', 'css.types.color.rgb.alpha_parameter', "CSS.supports('color', 'rgba(0,0,0,0.4)')"],
  ['css_rem', 'css.types.length.rem', "CSS.supports('font-size', '1.1rem')"],
  ['css_pre_wrap', 'css.properties.white-space.pre-wrap', "CSS.supports('white-space', 'pre-wrap')"],
  ['css_checked', 'css.selectors.checked', "CSS.supports('selector(:checked)')"],
  ['css_focus', 'css.selectors.focus', "CSS.supports('selector(:focus)')"],
  ['css_child', 'css.selectors.child', "CSS.supports('selector(a > b)')"],
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
  ['array_buffer_transfer', 'javascript.builtins.ArrayBuffer.transfer', any(call('transfer'), call('transferToFixedLength'))],
  ['start_view_transition', 'api.Document.startViewTransition', member('startViewTransition')],
  ['get_html', 'api.Element.getHTML', member('getHTML')],
  ['aria_braille_label', 'api.Element.ariaBrailleLabel', member('ariaBrailleLabel')],
  ['aria_braille_role_description', 'api.Element.ariaBrailleRoleDescription', member('ariaBrailleRoleDescription')],
  ['writing_suggestions', 'api.HTMLElement.writingSuggestions', word('writing[sS]uggestions')],
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
  ['dialog_request_close', 'api.HTMLDialogElement.requestClose', call('requestClose')],
  ['focus_visible_option', 'api.HTMLElement.focus.options_focusVisible_parameter', `\\bfocusVisible\\s*:|['"]focusVisible['"]\\s*:`],
  ['toggle_popover_options', 'api.HTMLElement.togglePopover.options_force_parameter', `(?:\\?\\.|\\.)togglePopover\\s*\\(\\s*\\{`],
  ['iterator_helpers', 'javascript.builtins.Iterator.map', any(`\\bIterator\\s*(?:\\?\\.|\\.)\\s*(?:from|concat|prototype)\\b`, `\\b(?:new|extends)\\s+Iterator\\b`, `\\b(?:values|keys|entries)\\s*\\(\\s*\\)\\s*(?:\\?\\.|\\.)\\s*${HELPERS}`, `\\bmatchAll\\s*\\([^()]*\\)\\s*(?:\\?\\.|\\.)\\s*${HELPERS}`, call('toArray'))],
  ['json_raw', 'javascript.builtins.JSON.rawJSON', any(fixed('JSON', 'rawJSON'), fixed('JSON', 'isRawJSON'))],
  ['show_popover_source', 'api.HTMLElement.showPopover.options_source_parameter', `(?:\\?\\.|\\.)showPopover\\s*\\(\\s*\\{`],
  ['custom_element_registry', 'api.Element.customElementRegistry', member('customElementRegistry')],
  ['command_invokers', 'api.HTMLButtonElement.commandForElement', any(member('commandForElement'), word('commandfor'), word('CommandEvent'), word('oncommand'), `addEventListener\\s*\\(\\s*['"]command['"]`, `setAttribute\\s*\\(\\s*['"]command['"]`, `(?:\\?\\.|\\.)command\\s*=(?!=)`)],
  ['scrollend_event', 'api.Element.scrollend_event', event('scrollend')],
  ['caret_position_from_point', 'api.Document.caretPositionFromPoint', member('caretPositionFromPoint')],
  ['aria_col_index_text', 'api.Element.ariaColIndexText', member('ariaColIndexText')],
  ['aria_row_index_text', 'api.Element.ariaRowIndexText', member('ariaRowIndexText')],
  ['map_get_or_insert', 'javascript.builtins.Map.getOrInsert', any(member('getOrInsert'), member('getOrInsertComputed'))],
  ['math_sum_precise', 'javascript.builtins.Math.sumPrecise', fixed('Math', 'sumPrecise')],
  ['window_navigation', 'api.Window.navigation', any(`\\bwindow\\s*(?:\\?\\.|\\.)\\s*navigation\\b`, `\\bnavigation\\s*(?:\\?\\.|\\.)\\s*(?:navigate|traverseTo|updateCurrentEntry|currentEntry|canGoBack|canGoForward|transition|activation)\\b`, word('on(?:navigate|currententrychange|navigatesuccess|navigateerror)'), `addEventListener\\s*\\(\\s*['"](?:navigate|currententrychange|navigatesuccess|navigateerror)['"]`)],
  ['current_css_zoom', 'api.Element.currentCSSZoom', member('currentCSSZoom')],
  ['aria_notify', 'api.Element.ariaNotify', member('ariaNotify')],
  ['pageswap_event', 'api.Window.pageswap_event', event('pageswap')],
  ['highlight_api', 'api.Highlight', any(`\\bnew\\s+Highlight\\b`, `\\bCSS\\s*(?:\\?\\.|\\.)\\s*highlights\\b`, `::highlight\\s*\\(`)],
  ['hidden_until_found', 'html.global_attributes.hidden.until-found', 'until-found'],
  ['css_starting_style', 'css.at-rules.starting-style', '@starting-style\\b'],
  ['css_transition_behavior', 'css.properties.transition-behavior', any(css('transition-behavior', 'transitionBehavior'), word('allow-discrete'))],
  ['css_light_dark', 'css.types.color.light-dark', `\\blight-dark\\s*\\(`],
  ['css_text_wrap', 'css.properties.text-wrap', `\\btext-wrap\\b|\\btextWrap`],
  ['css_content_visibility', 'css.properties.content-visibility', css('content-visibility', 'contentVisibility')],
  ['css_field_sizing', 'css.properties.field-sizing', css('field-sizing', 'fieldSizing')],
  ['css_scrollbar_gutter', 'css.properties.scrollbar-gutter', css('scrollbar-gutter', 'scrollbarGutter')],
  ['css_scrollbar_width', 'css.properties.scrollbar-width', css('scrollbar-width', 'scrollbarWidth')],
  ['css_scrollbar_color', 'css.properties.scrollbar-color', css('scrollbar-color', 'scrollbarColor')],
  ['css_text_box', 'css.properties.text-box', `\\btext-box\\b|\\btextBox`],
  ['css_contrast_color', 'css.types.color.contrast-color', `\\bcontrast-color\\s*\\(`],
  ['css_scope', 'css.at-rules.scope', '@scope\\b'],
];

/** The first Safari version, its date and the page of the data at `dotted`, or a problem: the
 * lowest version among the entries with no flag, prefix or other name, where a partial entry
 * counts only for a deny line, since the floor lacks the whole of such a feature too. */
export function lookup(data, dotted, kind = 'allow') {
  let node = data;
  for (const key of dotted.split('.')) node = node?.[key];
  const compat = node?.__compat;
  if (!compat) return { problem: `${dotted} is not in the data` };
  const raw = compat.support.safari;
  const versions = (Array.isArray(raw) ? raw : [raw])
    .filter((s) => s && !s.flags && !s.prefix && !s.alternative_name && (kind === 'deny' || !s.partial_implementation))
    .map((s) => s.version_added)
    .filter((v) => typeof v === 'string' && /^[0-9]+(?:\.[0-9]+)*$/.test(v))
    .sort(compareVersions);
  const version = versions[0];
  if (!version) return { problem: `${dotted} has no plain Safari version` };
  const release = data.browsers.safari.releases[version];
  if (!release?.release_date) return { problem: `${dotted}: Safari ${version} has no date in the data` };
  const page = compat.mdn_url ?? `https://developer.mozilla.org/ (${dotted})`;
  return { version, date: release.release_date, page };
}

/** The rows of U0.3: the allow rows and the deny rows. */
export const ROWS = { allow: ALLOW, deny: DENY };

/** The lines that `rows` make for the list from `data`, or the problems of the rows. */
export function lines(data, rows = ROWS) {
  const out = [];
  const problems = [];
  const emit = (kind, [name, dotted, cell]) => {
    const found = lookup(data, dotted, kind);
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
  for (const row of rows.allow) emit('allow', row);
  for (const row of rows.deny) emit('deny', row);
  return { out, problems };
}

/** The problems of `listText` against `data` and `rows`, with the count of feature lines read
 * and of those not read, because they have no row or no comment of the data above them. */
export function verify(data, listText, rows = ROWS) {
  const problems = [];
  const text = listText.split('\n');
  let read = 0;
  let unread = 0;
  for (const f of parseList(listText).features) {
    const m = /^# https:\/\/developer\.mozilla\.org\/(?:docs\/\S+| \(([a-z.A-Z_0-9-]+)\)), Safari (\S+) of (\S+)$/.exec(text[f.line - 2] ?? '');
    const row = [...rows.allow, ...rows.deny].find((r) => r[0] === f.name);
    if (!m || !row) {
      unread += 1;
      continue;
    }
    read += 1;
    const found = lookup(data, row[1], f.kind);
    if (found.problem) problems.push(found.problem);
    else if (found.version !== f.version || found.date !== m[3]) problems.push(`${f.name}: the list gives ${f.version} of ${m[3]}, the data ${found.version} of ${found.date}`);
  }
  const block = lines(data, rows).out.join('\n') + '\n';
  if (!listText.endsWith(block)) problems.push('the list does not end with the lines that the rows make from the data');
  return { problems, read, unread };
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
  let listText = null;
  if (flag === '--verify') {
    try {
      listText = fs.readFileSync(listFile, 'utf8');
    } catch {
      process.stderr.write(`REFUSED usage: the list ${path.basename(listFile)} cannot be read\n`);
      return 2;
    }
  }
  const { out, problems } = lines(data);
  let counts = null;
  if (listText !== null) {
    counts = verify(data, listText);
    problems.push(...counts.problems);
  } else if (problems.length === 0) {
    process.stdout.write(out.join('\n') + '\n');
  }
  for (const p of problems) process.stdout.write(`miss ${p}\n`);
  if (counts) process.stdout.write(`verify rows=${ALLOW.length + DENY.length} read=${counts.read} unread=${counts.unread} problems=${problems.length}\n`);
  return problems.length === 0 ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
