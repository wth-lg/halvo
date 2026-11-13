(function () {
  'use strict';
  var FEATURES = [
  ["dialog_element", function () { return (typeof HTMLDialogElement === 'function'); }],
  ["dialog_show_modal", function () { return (typeof HTMLDialogElement.prototype.showModal === 'function'); }],
  ["dialog_cancel_event", function () { return ('oncancel' in HTMLDialogElement.prototype); }],
  ["dialog_close_event", function () { return ('onclose' in HTMLDialogElement.prototype); }],
  ["autofocus_property", function () { return ('autofocus' in HTMLElement.prototype); }],
  ["tab_index", function () { return ('tabIndex' in HTMLElement.prototype); }],
  ["backdrop_selector", function () { return (CSS.supports('selector(::backdrop)')); }],
  ["overflow_wrap_anywhere", function () { return (CSS.supports('overflow-wrap', 'anywhere')); }],
  ["appearance_none", function () { return (CSS.supports('appearance', 'none')); }],
  ["flex_gap", function () { return ((function () { var box = document.createElement('div'); box.style.display = 'flex'; box.style.flexDirection = 'column'; box.style.rowGap = '1px'; box.appendChild(document.createElement('div')); box.appendChild(document.createElement('div')); document.documentElement.appendChild(box); var gap = box.scrollHeight === 1; document.documentElement.removeChild(box); return gap; })()); }],
  ["keydown_event", function () { return ('onkeydown' in window); }],
  ["keyup_event", function () { return ('onkeyup' in window); }],
  ["keyboard_event_key", function () { return ('key' in KeyboardEvent.prototype); }],
  ["keyboard_event_repeat", function () { return ('repeat' in KeyboardEvent.prototype); }],
  ["pointerdown_event", function () { return ('onpointerdown' in window); }],
  ["pointerup_event", function () { return ('onpointerup' in window); }],
  ["pointercancel_event", function () { return ('onpointercancel' in window); }],
  ["click_count_detail", function () { return ('detail' in UIEvent.prototype); }],
  ["window_focus_event", function () { return ('onfocus' in window); }],
  ["node_is_connected", function () { return ('isConnected' in Node.prototype); }],
  ["string_code_point_at", function () { return ('codePointAt' in String.prototype); }],
  ["map_and_set", function () { return (typeof Map === 'function' && typeof Set === 'function'); }],
  ["promise", function () { return (typeof Promise === 'function'); }],
  ["array_from", function () { return (typeof Array.from === 'function'); }],
  ["intl_segmenter", function () { return (typeof Intl.Segmenter === 'function' && typeof new Intl.Segmenter(undefined, { granularity: 'grapheme' }).segment === 'function'); }],
  ["click_event", function () { return ('onclick' in window); }],
  ["input_event", function () { return ('oninput' in window); }],
  ["scroll_top", function () { return ('scrollTop' in Element.prototype); }],
  ["focus_method", function () { return (typeof HTMLElement.prototype.focus === 'function'); }],
  ["node_contains", function () { return (typeof Node.prototype.contains === 'function'); }],
  ["element_remove", function () { return (typeof Element.prototype.remove === 'function'); }],
  ["create_element", function () { return (typeof document.createElement === 'function'); }],
  ["create_text_node", function () { return (typeof document.createTextNode === 'function'); }],
  ["append_child", function () { return (typeof Node.prototype.appendChild === 'function'); }],
  ["insert_before", function () { return (typeof Node.prototype.insertBefore === 'function'); }],
  ["remove_child", function () { return (typeof Node.prototype.removeChild === 'function'); }],
  ["first_child", function () { return ('firstChild' in Node.prototype); }],
  ["next_sibling", function () { return ('nextSibling' in Node.prototype); }],
  ["set_attribute", function () { return (typeof Element.prototype.setAttribute === 'function'); }],
  ["get_attribute", function () { return (typeof Element.prototype.getAttribute === 'function'); }],
  ["has_attribute", function () { return (typeof Element.prototype.hasAttribute === 'function'); }],
  ["query_selector_all", function () { return (typeof Element.prototype.querySelectorAll === 'function'); }],
  ["class_list", function () { return ('classList' in Element.prototype && typeof DOMTokenList.prototype.add === 'function'); }],
  ["class_name", function () { return ('className' in Element.prototype); }],
  ["tag_name", function () { return ('tagName' in Element.prototype); }],
  ["active_element", function () { return ('activeElement' in Document.prototype); }],
  ["add_event_listener", function () { return (typeof EventTarget.prototype.addEventListener === 'function'); }],
  ["prevent_default", function () { return (typeof Event.prototype.preventDefault === 'function'); }],
  ["event_target", function () { return ('target' in Event.prototype); }],
  ["event_current_target", function () { return ('currentTarget' in Event.prototype); }],
  ["dialog_open", function () { return ('open' in HTMLDialogElement.prototype); }],
  ["dialog_close", function () { return (typeof HTMLDialogElement.prototype.close === 'function'); }],
  ["input_checked", function () { return ('checked' in HTMLInputElement.prototype); }],
  ["input_value", function () { return ('value' in HTMLInputElement.prototype); }],
  ["style_element", function () { return (typeof HTMLStyleElement === 'function'); }],
  ["json_stringify", function () { return (typeof JSON.stringify === 'function'); }],
  ["array_is_array", function () { return (typeof Array.isArray === 'function'); }],
  ["array_methods", function () { return (['forEach', 'map', 'filter', 'some', 'every', 'reduce', 'indexOf', 'join', 'push'].every(function (m) { return typeof Array.prototype[m] === 'function'; })); }],
  ["string_from_char_code", function () { return (typeof String.fromCharCode === 'function'); }],
  ["string_trim", function () { return (typeof String.prototype.trim === 'function'); }],
  ["promise_methods", function () { return (typeof Promise.resolve === 'function' && typeof Promise.prototype.then === 'function' && typeof Promise.prototype.catch === 'function'); }],
  ["map_and_set_methods", function () { return (['get', 'set', 'has', 'delete', 'keys', 'values', 'forEach'].every(function (m) { return typeof Map.prototype[m] === 'function'; }) && ['add', 'has', 'delete', 'forEach'].every(function (m) { return typeof Set.prototype[m] === 'function'; })); }],
  ["event_type", function () { return ('type' in Event.prototype); }],
  ["element_id", function () { return ('id' in Element.prototype); }],
  ["input_type", function () { return ('type' in HTMLInputElement.prototype); }],
  ["input_name", function () { return ('name' in HTMLInputElement.prototype); }],
  ["button_type", function () { return ('type' in HTMLButtonElement.prototype); }],
  ["node_list", function () { return ('length' in NodeList.prototype && typeof NodeList.prototype.item === 'function'); }],
  ["string_function", function () { return (typeof String === 'function' && String(1) === '1'); }],
  ["boolean_function", function () { return (typeof Boolean === 'function' && Boolean(1) === true); }],
  ["symbol_iterator", function () { return (typeof Symbol === 'function' && typeof Symbol.iterator === 'symbol' && typeof ''[Symbol.iterator] === 'function' && typeof new Map().values()[Symbol.iterator] === 'function'); }],
  ["css_flex", function () { return (CSS.supports('display', 'flex') && CSS.supports('flex-wrap', 'wrap') && CSS.supports('align-items', 'center')); }],
  ["css_inline_block", function () { return (CSS.supports('display', 'inline-block')); }],
  ["css_border_radius", function () { return (CSS.supports('border-radius', '50%')); }],
  ["css_box_sizing", function () { return (CSS.supports('box-sizing', 'border-box')); }],
  ["css_outline_offset", function () { return (CSS.supports('outline-offset', '2px')); }],
  ["css_rgba_function", function () { return (CSS.supports('color', 'rgba(0,0,0,0.4)')); }],
  ["css_rem", function () { return (CSS.supports('font-size', '1.1rem')); }],
  ["css_pre_wrap", function () { return (CSS.supports('white-space', 'pre-wrap')); }],
  ["css_checked", function () { return (CSS.supports('selector(:checked)')); }],
  ["css_focus", function () { return (CSS.supports('selector(:focus)')); }],
  ["css_child", function () { return (CSS.supports('selector(a > b)')); }]
  ];
  var missing = [];
  FEATURES.forEach(function (feature) {
    var present = false;
    try {
      present = feature[1]() === true;
    } catch (e) {
      present = false;
    }
    if (!present) missing.push(feature[0]);
  });
  var agent = String(navigator.userAgent);
  var webkit = /AppleWebKit\/([0-9.]+)/.exec(agent);
  var edge = /Edg\/([0-9.]+)/.exec(agent);
  var aria = typeof document.ariaNotify === 'function' ? 'function' : 'undefined';
  window.r4app.report('probe features=' + FEATURES.length + ' missing=' + missing.length +
    ' missing_names=' + (missing.length ? missing.join(',') : '-') +
    ' webkit=' + (webkit ? webkit[1] : '-') + ' edg=' + (edge ? edge[1] : '-') + ' aria_notify=' + aria);
})();
