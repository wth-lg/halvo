"use strict";

const VOID = new Set(["meta", "link", "input", "br", "img", "hr", "source", "wbr"]);
const RAW = new Set(["script", "style"]);
const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ", mdash: "—", ndash: "–" };

function decode(text) {
  return text.replace(/&(#x[0-9a-f]+|#[0-9]+|[a-z]+);/gi, (all, name) => {
    if (name[0] === "#") {
      const code = name[1].toLowerCase() === "x" ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
      return String.fromCodePoint(code);
    }
    return ENTITIES[name.toLowerCase()] ?? all;
  });
}

class Node {
  constructor(document) {
    this.ownerDocument = document;
    this.parentNode = null;
    this.childNodes = [];
  }
  get parentElement() {
    return this.parentNode instanceof Element ? this.parentNode : null;
  }
  get textContent() {
    return this.childNodes.map((n) => n.textContent).join("");
  }
  set textContent(value) {
    this.replaceChildren();
    const text = String(value ?? "");
    if (text !== "") this.appendChild(new Text(this.ownerDocument, text));
  }
  appendChild(node) {
    if (node.parentNode) node.parentNode.removeChild(node);
    node.parentNode = this;
    this.childNodes.push(node);
    return node;
  }
  removeChild(node) {
    const i = this.childNodes.indexOf(node);
    if (i < 0) throw new Error("not a child");
    this.childNodes.splice(i, 1);
    node.parentNode = null;
    return node;
  }
  insertBefore(node, ref) {
    if (ref === null || ref === undefined) return this.appendChild(node);
    if (node.parentNode) node.parentNode.removeChild(node);
    const i = this.childNodes.indexOf(ref);
    if (i < 0) throw new Error("not a child");
    node.parentNode = this;
    this.childNodes.splice(i, 0, node);
    return node;
  }
  append(...nodes) {
    for (const n of nodes) this.appendChild(typeof n === "string" ? new Text(this.ownerDocument, n) : n);
  }
  replaceChildren(...nodes) {
    for (const n of [...this.childNodes]) this.removeChild(n);
    this.append(...nodes);
  }
  remove() {
    if (this.parentNode) this.parentNode.removeChild(this);
  }
  contains(node) {
    for (let n = node; n; n = n.parentNode) if (n === this) return true;
    return false;
  }
  get children() {
    return this.childNodes.filter((n) => n instanceof Element);
  }
  get firstElementChild() {
    return this.children[0] ?? null;
  }
  *elements() {
    for (const child of this.children) {
      yield child;
      yield* child.elements();
    }
  }
  querySelectorAll(selector) {
    const match = compile(selector);
    return [...this.elements()].filter(match);
  }
  querySelector(selector) {
    return this.querySelectorAll(selector)[0] ?? null;
  }
}

class Text extends Node {
  constructor(document, data) {
    super(document);
    this.data = data;
  }
  get textContent() {
    return this.data;
  }
  set textContent(value) {
    this.data = String(value);
  }
}

class ClassList {
  constructor(element) {
    this.element = element;
  }
  get names() {
    return (this.element.getAttribute("class") || "").split(/\s+/).filter(Boolean);
  }
  write(names) {
    this.element.setAttribute("class", names.join(" "));
  }
  contains(name) {
    return this.names.includes(name);
  }
  add(...names) {
    this.write([...new Set([...this.names, ...names])]);
  }
  remove(...names) {
    this.write(this.names.filter((n) => !names.includes(n)));
  }
  toggle(name, force) {
    const on = force === undefined ? !this.contains(name) : Boolean(force);
    if (on) this.add(name);
    else this.remove(name);
    return on;
  }
}

const REFLECTED = { id: "id", className: "class", type: "type", min: "min", max: "max", step: "step", htmlFor: "for", lang: "lang", role: "role" };
const BOOLEAN = new Set(["disabled", "hidden"]);

class Element extends Node {
  constructor(document, tag) {
    super(document);
    this.tagName = tag.toUpperCase();
    this.localName = tag.toLowerCase();
    this.attributes = new Map();
    this.listeners = new Map();
    this.style = {};
    this.classList = new ClassList(this);
    this.onclick = null;
    this.oninput = null;
    this.onchange = null;
    this._value = null;
  }
  getAttribute(name) {
    return this.attributes.has(name) ? this.attributes.get(name) : null;
  }
  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }
  removeAttribute(name) {
    this.attributes.delete(name);
  }
  hasAttribute(name) {
    return this.attributes.has(name);
  }
  get value() {
    return this._value ?? this.getAttribute("value") ?? "";
  }
  set value(v) {
    this._value = String(v);
  }
  addEventListener(type, fn) {
    if (!this.listeners.has(type)) this.listeners.set(type, []);
    this.listeners.get(type).push(fn);
  }
  removeEventListener(type, fn) {
    const list = this.listeners.get(type) || [];
    const i = list.indexOf(fn);
    if (i >= 0) list.splice(i, 1);
  }
  dispatchEvent(event) {
    event.target = event.target || this;
    const results = [];
    for (let node = this; node && !event.stopped; node = node.parentNode) {
      if (!(node instanceof Element)) break;
      event.currentTarget = node;
      for (const fn of [...(node.listeners.get(event.type) || [])]) results.push(fn.call(node, event));
      const own = node["on" + event.type];
      if (typeof own === "function") results.push(own.call(node, event));
    }
    return Promise.all(results.map((r) => Promise.resolve(r)));
  }
  click(init = {}) {
    if (this.disabled) return Promise.resolve([]);
    return this.dispatchEvent(makeEvent("click", { detail: 1, ...init }));
  }
  async press(init = {}) {
    if (this.disabled) return [];
    for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup"]) await this.dispatchEvent(makeEvent(type, { detail: 1, ...init }));
    return this.dispatchEvent(makeEvent("click", { detail: 1, ...init }));
  }
  getBoundingClientRect() {
    const r = this.rect || { left: 0, top: 0, width: 0, height: 0 };
    return { left: r.left, top: r.top, width: r.width, height: r.height, right: r.left + r.width, bottom: r.top + r.height, x: r.left, y: r.top };
  }
  focus() {
    this.ownerDocument.activeElement = this;
  }
  blur() {
    if (this.ownerDocument.activeElement === this) this.ownerDocument.activeElement = null;
  }
  closest(selector) {
    const match = compile(selector);
    for (let n = this; n instanceof Element; n = n.parentNode) if (match(n)) return n;
    return null;
  }
  get isConnected() {
    return this.ownerDocument.documentElement.contains(this);
  }
}

for (const [prop, attr] of Object.entries(REFLECTED)) {
  Object.defineProperty(Element.prototype, prop, {
    get() {
      return this.getAttribute(attr) ?? "";
    },
    set(v) {
      this.setAttribute(attr, v);
    },
  });
}
for (const prop of BOOLEAN) {
  Object.defineProperty(Element.prototype, prop, {
    get() {
      return this.hasAttribute(prop);
    },
    set(v) {
      if (v) this.setAttribute(prop, "");
      else this.removeAttribute(prop);
    },
  });
}

function makeEvent(type, init = {}) {
  return {
    type,
    detail: 0,
    ...init,
    stopped: false,
    defaultPrevented: false,
    stopPropagation() {
      this.stopped = true;
    },
    preventDefault() {
      this.defaultPrevented = true;
    },
  };
}

function compile(selector) {
  const alternatives = selector.split(",").map((s) => s.trim());
  const tests = alternatives.map((one) => {
    const parts = one.match(/^([a-z0-9]*)((?:[#.][\w-]+|\[[\w-]+(?:="[^"]*")?\])*)$/i);
    if (!parts) throw new Error(`the stub DOM does not read the selector ${one}`);
    const tag = parts[1].toLowerCase();
    const rest = [...parts[2].matchAll(/#([\w-]+)|\.([\w-]+)|\[([\w-]+)(?:="([^"]*)")?\]/g)];
    return (el) =>
      (!tag || el.localName === tag) &&
      rest.every(([, id, cls, attr, value]) =>
        id ? el.id === id : cls ? el.classList.contains(cls) : value === undefined ? el.hasAttribute(attr) : el.getAttribute(attr) === value,
      );
  });
  return (el) => tests.some((t) => t(el));
}

class Document extends Node {
  constructor() {
    super(null);
    this.ownerDocument = this;
    this.activeElement = null;
    this.listeners = new Map();
    this.visibilityState = "visible";
    this.readyState = "loading";
    this.documentElement = this.createElement("html");
    this.appendChild(this.documentElement);
  }
  get hidden() {
    return this.visibilityState !== "visible";
  }
  createElement(tag) {
    return new Element(this, tag);
  }
  createTextNode(text) {
    return new Text(this, String(text));
  }
  getElementById(id) {
    for (const el of this.elements()) if (el.id === id) return el;
    return null;
  }
  get head() {
    return this.querySelector("head");
  }
  get body() {
    return this.querySelector("body");
  }
  addEventListener(type, fn) {
    if (!this.listeners.has(type)) this.listeners.set(type, []);
    this.listeners.get(type).push(fn);
  }
  removeEventListener(type, fn) {
    const list = this.listeners.get(type) || [];
    const i = list.indexOf(fn);
    if (i >= 0) list.splice(i, 1);
  }
  dispatchEvent(event) {
    event.target = event.target || this;
    for (const fn of [...(this.listeners.get(event.type) || [])]) fn.call(this, event);
  }
}

function parse(html) {
  const document = new Document();
  const scripts = [];
  let current = document.documentElement;
  let i = 0;
  const open = (tag, attrs) => {
    if (tag === "html") {
      for (const [k, v] of attrs) current.setAttribute(k, v);
      return;
    }
    const el = document.createElement(tag);
    for (const [k, v] of attrs) el.setAttribute(k, v);
    current.appendChild(el);
    if (tag === "script") scripts.push(el);
    if (!VOID.has(tag)) current = el;
  };
  while (i < html.length) {
    if (html.startsWith("<!--", i)) {
      const end = html.indexOf("-->", i);
      i = end < 0 ? html.length : end + 3;
    } else if (html.startsWith("<!", i)) {
      i = html.indexOf(">", i) + 1;
    } else if (html.startsWith("</", i)) {
      const end = html.indexOf(">", i);
      const tag = html.slice(i + 2, end).trim().toLowerCase();
      for (let n = current; n instanceof Element; n = n.parentNode) {
        if (n.localName === tag) {
          current = n.parentNode instanceof Element ? n.parentNode : document.documentElement;
          break;
        }
      }
      i = end + 1;
    } else if (html[i] === "<" && /[a-z]/i.test(html[i + 1] || "")) {
      const m = /^<([a-z0-9-]+)((?:\s+[^\s=/>]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)\s*(\/?)>/i.exec(html.slice(i));
      if (!m) throw new Error(`the stub DOM cannot read the tag at offset ${i}`);
      const tag = m[1].toLowerCase();
      const attrs = [...m[2].matchAll(/([^\s=/>]+)(?:\s*=\s*("([^"]*)"|'([^']*)'|[^\s>]+))?/g)].map((a) => [
        a[1].toLowerCase(),
        decode(a[3] ?? a[4] ?? (a[2] && !/^["']/.test(a[2]) ? a[2] : "")),
      ]);
      i += m[0].length;
      open(tag, attrs);
      if (RAW.has(tag)) {
        const end = html.toLowerCase().indexOf(`</${tag}`, i);
        const body = html.slice(i, end < 0 ? html.length : end);
        if (body) current.appendChild(new Text(document, body));
        current = current.parentNode;
        i = end < 0 ? html.length : html.indexOf(">", end) + 1;
      } else if (m[3] === "/" && !VOID.has(tag)) {
        current = current.parentNode;
      }
    } else {
      const next = html.indexOf("<", i + 1);
      const text = html.slice(i, next < 0 ? html.length : next);
      if (text.trim()) current.appendChild(new Text(document, decode(text)));
      i = next < 0 ? html.length : next;
    }
  }
  return { document, scripts };
}

module.exports = { parse, makeEvent, Element, Document };
