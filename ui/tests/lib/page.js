"use strict";

const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { parse, makeEvent } = require("./dom.js");

const UI = path.resolve(__dirname, "..", "..");
const APP = path.join(UI, "app");
const FIXTURES = path.join(UI, "tests", "fixtures", "capture");

function fixture(name) {
  return JSON.parse(fs.readFileSync(path.join(FIXTURES, `${name}.json`), "utf8"));
}

function calls() {
  return fixture("calls");
}

const settle = async () => {
  for (let i = 0; i < 30; i++) await new Promise((resolve) => setImmediate(resolve));
};

class Clock {
  constructor() {
    this.now = 0;
    this.timers = [];
    this.serial = 0;
  }
  add(fn, ms, repeat) {
    const id = ++this.serial;
    const period = Math.max(0, Number(ms) || 0);
    this.timers.push({ id, fn, period, due: this.now + period, repeat });
    return id;
  }
  clear(id) {
    this.timers = this.timers.filter((t) => t.id !== id);
  }
  periods() {
    return this.timers.filter((t) => t.repeat).map((t) => t.period);
  }
  async run(timer) {
    if (timer.repeat) timer.due += Math.max(timer.period, 1);
    else this.clear(timer.id);
    await timer.fn();
    await settle();
  }
  async advance(ms) {
    const end = this.now + ms;
    for (;;) {
      const due = this.timers.filter((t) => t.due <= end).sort((a, b) => a.due - b.due || a.id - b.id)[0];
      if (!due) break;
      this.now = Math.max(this.now, due.due);
      await this.run(due);
    }
    this.now = end;
    await settle();
  }
  async fire(ms) {
    for (const timer of this.timers.filter((t) => t.repeat && t.period === ms)) {
      await timer.fn();
      await settle();
    }
  }
}

function defaultReplies() {
  return {
    capture_status: () => fixture("capture_status-running"),
    inventory: () => fixture("inventory-empty"),
    onboarding_state: () => fixture("onboarding_state-none"),
    last_delete: () => fixture("last_delete-none"),
    "plugin:app|version": () => "0.0.0",
  };
}

async function openPage(options = {}) {
  const { dir = APP, replies = {}, globalTauri = false, timezoneOffset } = options;
  const html = fs.readFileSync(path.join(dir, "index.html"), "utf8");
  const { document, scripts } = parse(html);
  const clock = new Clock();
  const sent = [];
  const handlers = { ...defaultReplies(), ...replies };
  const invoke = (cmd, args) => {
    sent.push({ cmd, args: args === undefined ? undefined : JSON.parse(JSON.stringify(args)), at: clock.now });
    const handler = handlers[cmd];
    if (handler === undefined) return Promise.reject(`no reply for ${cmd}`);
    try {
      const value = typeof handler === "function" ? handler(args) : handler;
      return Promise.resolve(value).then((v) => (v === undefined ? null : JSON.parse(JSON.stringify(v))));
    } catch (error) {
      return Promise.reject(error);
    }
  };
  const context = {
    console,
    document,
    performance: { now: () => clock.now },
    setTimeout: (fn, ms) => clock.add(fn, ms, false),
    setInterval: (fn, ms) => clock.add(fn, ms, true),
    clearTimeout: (id) => clock.clear(id),
    clearInterval: (id) => clock.clear(id),
    __TAURI_INTERNALS__: { invoke },
    addEventListener: () => {},
  };
  const loads = { again: 0 };
  context.location = {
    hash: options.hash || "",
    pathname: "/",
    search: "",
    reload: () => {
      loads.again += 1;
    },
  };
  context.history = {
    replaceState: (state, title, url) => {
      context.location.hash = String(url).includes("#") ? String(url).slice(String(url).indexOf("#")) : "";
    },
  };
  if (globalTauri) context.__TAURI__ = { core: { invoke } };
  context.window = context;
  context.self = context;
  vm.createContext(context);
  const nowMs = options.nowMs === undefined ? KEPT_AT_MS + 60000 : options.nowMs;
  vm.runInContext(
    `(function () { var D = Date; var NOW = ${Number(nowMs)}; globalThis.Date = class extends D { constructor(...a) { if (a.length === 0) super(NOW); else super(...a); } static now() { return NOW; } }; })()`,
    context,
  );
  if (timezoneOffset !== undefined) {
    vm.runInContext(`(function () { var D = Date; globalThis.Date = class extends D { getTimezoneOffset() { return ${Number(timezoneOffset)}; } }; })()`, context);
  }
  const loaded = [];
  for (const script of scripts) {
    const src = script.getAttribute("src");
    const file = src ? path.join(dir, src) : null;
    const code = src ? fs.readFileSync(file, "utf8") : script.textContent;
    loaded.push(src || "(inline)");
    vm.runInContext(code, context, { filename: file || path.join(dir, "index.html") });
  }
  document.readyState = "interactive";
  document.dispatchEvent(makeEvent("DOMContentLoaded"));
  await settle();
  return makePage({ document, clock, sent, context, loaded, dir, loads, reopen: () => openPage({ ...options, hash: context.location.hash }) });
}

function makePage({ document, clock, sent, context, loaded, dir, loads, reopen }) {
  const byId = (id) => {
    const el = document.getElementById(id);
    if (!el) throw new Error(`the page has no element #${id}`);
    return el;
  };
  const hiddenBy = (el) => {
    for (let n = el; n && n.classList; n = n.parentNode) if (n.classList.contains("hidden") || n.hidden) return true;
    return false;
  };
  return {
    document,
    clock,
    sent,
    window: context,
    loaded,
    dir,
    byId,
    text: (id) => byId(id).textContent,
    visible: (target) => !hiddenBy(typeof target === "string" ? byId(target) : target),
    calls: (cmd) => sent.filter((c) => c.cmd === cmd),
    loadsAgain: () => loads.again,
    reopen,
    settle,
    async click(target, init) {
      const el = typeof target === "string" ? byId(target) : target;
      await el.click(init);
      await settle();
    },
    async press(target, init) {
      const el = typeof target === "string" ? byId(target) : target;
      await el.press(init);
      await settle();
    },
    /** A press as a web view makes it: the pressed control takes the focus first (AP-17 G4). */
    async pressWithFocus(target, init) {
      const el = typeof target === "string" ? byId(target) : target;
      el.focus();
      await el.press(init);
      await settle();
    },
    async pressDown(target, init) {
      const el = typeof target === "string" ? byId(target) : target;
      await el.dispatchEvent(makeEvent("pointerdown", { detail: 1, ...init }));
      await el.dispatchEvent(makeEvent("mousedown", { detail: 1, ...init }));
      await settle();
    },
    async type(id, value) {
      const el = byId(id);
      el.focus();
      el.value = value;
      await el.dispatchEvent(makeEvent("input"));
      await settle();
    },
    async hide() {
      document.visibilityState = "hidden";
      document.dispatchEvent(makeEvent("visibilitychange"));
      await settle();
    },
  };
}

/** The app's side of a delete for a page test: the answer is kept with its time, and last_delete
 * gives it to the page that loads again, as the app does (ruling AP-14 L4). */
const KEPT_AT_MS = Date.UTC(2026, 10, 14, 9, 30);

function keptDelete(replies = {}, atMs = KEPT_AT_MS) {
  let kept = { state: "none" };
  const answer = replies.delete_all;
  return {
    ...replies,
    delete_all: (args) => {
      kept = { state: "in_flight" };
      return Promise.resolve()
        .then(() => (typeof answer === "function" ? answer(args) : answer))
        .then(
          (value) => {
            kept = { state: "answered", at_ms: atMs, answer: JSON.parse(JSON.stringify(value)) };
            return value;
          },
          (error) => {
            kept = { state: "none" };
            throw error;
          },
        );
    },
    last_delete: () => kept,
  };
}

async function loadedAgain(page) {
  if (page.loadsAgain() !== 1) throw new Error(`the page loaded again ${page.loadsAgain()} times after the delete`);
  return page.reopen();
}

module.exports = { openPage, fixture, calls, settle, Clock, UI, APP, FIXTURES, keptDelete, loadedAgain, KEPT_AT_MS };
