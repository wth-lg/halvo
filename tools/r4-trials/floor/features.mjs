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
const call = (m) => `(?:\\?\\.|\\.)${m}\\s*\\(|\\[\\s*['"]${m}['"]\\s*\\]\\s*\\(`;
const callWith = (m, open) => `(?:\\?\\.|\\.)${m}\\s*\\(\\s*${open}|\\[\\s*['"]${m}['"]\\s*\\]\\s*\\(\\s*${open}`;
const taken = (s, m) => `\\{[^{}]{0,200}\\b${m}\\b[^{}]{0,200}\\}\\s*=\\s*${s}\\b`;
const fixed = (s, m) => `\\b${s}\\s*(?:\\?\\.|\\.)\\s*${m}\\b|\\b${s}\\s*(?:\\?\\.)?\\s*\\[\\s*['"]${m}['"]\\s*\\]|${taken(s, m)}`;
const css = (property, js) => `\\b${property}\\b|\\b${js}\\b`;
const event = (e) => `['"]${e}['"]|\\bon${e}\\b`;
const word = (w) => `\\b${w}\\b`;
const any = (...parts) => parts.join('|');
const DECORATION = `[^;}'"]{0,200}?(?:\\b(?:solid|double|dotted|dashed|wavy|currentColor|transparent)\\b|#[0-9a-fA-F]{3,8}\\b|\\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\\s*\\()`;
const SPACES = '(?:collapse|preserve-breaks|preserve|wrap)\\b';
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
  ['css_rgba_function', 'css.types.color.rgb', "CSS.supports('color', 'rgba(0,0,0,0.4)')"],
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
  ['start_view_transition', 'api.Document.startViewTransition', any(member('startViewTransition'), taken('document', 'startViewTransition'))],
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
  ['focus_visible_option', 'api.HTMLElement.focus.options_focusVisible_parameter', word('focusVisible')],
  ['toggle_popover_options', 'api.HTMLElement.togglePopover.options_force_parameter', callWith('togglePopover', '\\{')],
  ['iterator_helpers', 'javascript.builtins.Iterator.map', any(`\\bIterator\\s*(?:\\?\\.|\\.)\\s*(?:from|concat|prototype)\\b`, `\\b(?:new|extends)\\s+Iterator\\b`, `\\b(?:values|keys|entries)\\s*\\(\\s*\\)\\s*(?:\\?\\.|\\.)\\s*${HELPERS}`, `\\bmatchAll\\s*\\([^()]*\\)\\s*(?:\\?\\.|\\.)\\s*${HELPERS}`, call('toArray'))],
  ['json_raw', 'javascript.builtins.JSON.rawJSON', any(fixed('JSON', 'rawJSON'), fixed('JSON', 'isRawJSON'))],
  ['show_popover_source', 'api.HTMLElement.showPopover.options_source_parameter', callWith('showPopover', '\\{')],
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
  ['css_text_wrap', 'css.properties.text-wrap', `\\btext-wrap\\b|\\btextWrap(?:Mode|Style)?\\b`],
  ['css_content_visibility', 'css.properties.content-visibility', css('content-visibility', 'contentVisibility')],
  ['css_field_sizing', 'css.properties.field-sizing', css('field-sizing', 'fieldSizing')],
  ['css_scrollbar_gutter', 'css.properties.scrollbar-gutter', css('scrollbar-gutter', 'scrollbarGutter')],
  ['css_scrollbar_width', 'css.properties.scrollbar-width', css('scrollbar-width', 'scrollbarWidth')],
  ['css_scrollbar_color', 'css.properties.scrollbar-color', css('scrollbar-color', 'scrollbarColor')],
  ['css_text_box', 'css.properties.text-box', `\\btext-box\\b|\\btextBox(?:Trim|Edge)?\\b`],
  ['css_contrast_color', 'css.types.color.contrast-color', `\\bcontrast-color\\s*\\(`],
  ['css_scope', 'css.at-rules.scope', '@scope\\b'],
  ['css_backdrop_filter', 'css.properties.backdrop-filter', `(?<![\\w-])backdrop-filter\\b|\\bbackdropFilter\\b`],
  ['css_counter_set', 'css.properties.counter-set', css('counter-set', 'counterSet')],
  ['details_name', 'html.elements.details.name', any(`\\bdetails\\s*(?:\\?\\.|\\.)name\\s*=(?!=)`, `\\bdetails\\s*(?:\\?\\.|\\.)setAttribute\\s*\\(\\s*['"]name['"]`, `<details\\b[^>]{0,200}\\sname\\s*=`, String.raw`\b([A-Za-z_$][\w$]*)\s*=\s*[^;\n]{0,200}\(\s*['"]details['"][^;\n]{0,200}[;\n][\s\S]{0,4000}?\b\1\s*(?:\?\.|\.)(?:name\s*=(?!=)|setAttribute\s*\(\s*['"]name['"])`)],
  ['css_white_space_collapse', 'css.properties.white-space-collapse', css('white-space-collapse', 'whiteSpaceCollapse')],
  ['css_view_transition_name', 'css.properties.view-transition-name', css('view-transition-name', 'viewTransitionName')],
  ['css_mask_border', 'css.properties.mask-border', `(?<![\\w-])mask-border\\b|\\bmaskBorder\\b`],
  ['css_state_pseudo', 'css.selectors.state', `:state\\s*\\(`],
  ['css_target_text', 'css.selectors.target-text', `::target-text\\b`],
  ['element_internals_states', 'api.ElementInternals.states', any(`\\binternals\\s*(?:\\?\\.|\\.)states\\b`, `\\battachInternals\\s*\\(\\s*\\)\\s*(?:\\?\\.|\\.)states\\b`, word('CustomStateSet'))],
  ['url_parse', 'api.URL.parse_static', fixed('URL', 'parse')],
  ['css_open_pseudo', 'css.selectors.open', `:open\\b`],
  ['css_anchor_name', 'css.properties.anchor-name', css('anchor-name', 'anchorName')],
  ['css_position_anchor', 'css.properties.position-anchor', css('position-anchor', 'positionAnchor')],
  ['css_position_area', 'css.properties.position-area', css('position-area', 'positionArea')],
  ['css_animation_timeline', 'css.properties.animation-timeline', css('animation-timeline', 'animationTimeline')],
  ['css_linear_easing', 'css.types.easing-function.linear-function', `\\blinear\\s*\\(`],
  ['input_switch', 'html.elements.input.switch', any(`setAttribute\\s*\\(\\s*['"]switch['"]`, `(?:\\?\\.|\\.)switch\\s*=(?!=)`, `<input\\b[^>]{0,200}\\sswitch\\b`)],
  ['array_buffer_detached', 'javascript.builtins.ArrayBuffer.detached', member('detached')],
  ['css_font_width', 'css.properties.font-width', css('font-width', 'fontWidth')],
  ['css_text_decoration_style', 'css.properties.text-decoration.includes_color-and-style', any(`(?<![\\w-])text-decoration\\s*:${DECORATION}`, `\\btextDecoration\\s*=\\s*['"]${DECORATION}`)],
  ['css_safe_alignment', 'css.properties.align-items.safe', `\\b(?:un)?safe\\s+(?:center|start|end|flex-start|flex-end|self-start|self-end|left|right|baseline)\\b`],
  ['css_white_space_values', 'css.properties.white-space.collapse', any(`\\bwhite-space\\s*:\\s*${SPACES}`, `\\bwhiteSpace\\s*=\\s*['"]${SPACES}`)],
];

/** The comment lines above the deny lines: what the patterns do not hold. */
export const LIMITS = [
  '# The deny patterns read the text of a view file. They do not hold a global or a method held under another name (const O = Object; O.groupBy, const n = navigation),',
  '# an iterator kept in a variable before a helper is called on it, an event name put together from parts, a comment between an object and its member, or a \\u escape inside a name.',
  '# A view may not give its own members the names .command =, .transfer(, .toArray( or .detached, nor call a variable textBox or a class text-box: the patterns refuse them,',
  '# because a narrower pattern would let the denied feature pass.',
  '# details_name follows a variable that a view sets to a call whose first argument is \'details\', and a variable named details; it does not follow a details element',
  '# that is passed in, found by a query or made where no such call names it, nor tell such an element from another object that has a name.',
  '# The check does not hold accent-color, relaxed CSS nesting or the relative colour form rgb(from ...), because the floor has a partial form of each that a pattern cannot tell apart,',
  '# nor a text-decoration shorthand whose colour is a colour name.',
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
  out.push('# U0.3 wrote the allow lines below for each web API that the two views call and that had no line, and for some of the CSS that they use: with the lines above,');
  out.push('# 12 of the 47 CSS properties that the views\' style strings set have an allow line, and 35 have none (rulings RD-27 (b), RD-30 (6));');
  out.push('# and the deny lines after them for the features that reached Safari after the floor and that a view could use.');
  out.push(`# Each version and date below was read on 2026-11-12 from @mdn/browser-compat-data ${data.__meta.version} of ${data.__meta.timestamp.slice(0, 10)}, whose package states the licence CC0-1.0;`);
  out.push('# no file of that package is in this repository: tools/r4-trials/floor/features.mjs makes these lines from it and checks them against it.');
  out.push('# No page ran on macOS 14 itself: the cells of docs/r4/trials/floor.md ran on hosted macOS 15 and on hosted Windows, so for the macOS versions between the floor');
  out.push('# and the oldest cell the rule of this list alone stands, and the deny patterns are a guard and no proof.');
  for (const row of rows.allow) emit('allow', row);
  if (rows.deny.length > 0) out.push(...LIMITS);
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
