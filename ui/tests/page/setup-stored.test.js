"use strict";

const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const assert = require("node:assert/strict");
const { openPage, UI } = require("../lib/page.js");
const { makeEvent } = require("../lib/dom.js");

const COORD = path.join(UI, "tests", "fixtures", "coord");
const read = (name) => JSON.parse(fs.readFileSync(path.join(COORD, name), "utf8"));
const words = read("words.json");
const setup = read("state-needs_setup.json").snapshot;

const HINTS = { workbook: "workbook_mac", sheet: "sheet", range: "range", source: "source_mac" };
const FILES = ["workbook", "source"];
const STORED = {
  workbook: "/Volumes/work/<b>Budget</b>.xlsx",
  sheet: "Sheet1",
  range: "A2:C11",
  source: "/Volumes/work/receipts.csv",
};

/** An entry of a typed question of the spreadsheet agent as the core writes it, with the key `stored` where
 * `stored` holds the question's stored line (r4 U3.22g). */
function entry(name, stored) {
  const e = {
    name,
    label: words["param." + name],
    candidates: [],
    typed: "yes",
    full: words["param_typed.full"],
    too_long: words["param_typed.too_long"],
    hint: words["param_hint." + HINTS[name]],
    needs_answer: words["param_typed.needs_answer"],
  };
  const chooser = FILES.includes(name) ? { ...e, choose: words["param_typed.choose"] } : e;
  return stored ? { ...chooser, stored: STORED[name] } : chooser;
}

const [base] = setup.items;
const storedItem = { ...base, params: Object.keys(HINTS).map((n) => entry(n, true)) };
const emptyItem = { ...base, params: Object.keys(HINTS).map((n) => entry(n, false)) };

const lineOf = (page, name) => page.document.querySelector(`input.hn-typed[data-param="${name}"]`);
const setOf = (page, name) => page.document.querySelectorAll("fieldset.hn-param")[Object.keys(HINTS).indexOf(name)];
const useAnswers = (page) => page.document.querySelector('button[data-act="choose"]');

/** Which sheet as the core writes it behind a workbook whose names were read (r4 U3.22c). */
function listEntry(names) {
  return {
    name: "sheet",
    label: words["param.sheet"],
    candidates: names.map((value, i) => ({ n: i + 1, value: [value], from: "files" })),
    typed: "no",
    full: "",
    too_long: "",
    hint: "",
    needs_answer: words["param_typed.needs_answer"],
  };
}

const radiosOf = (page, name) => page.document.querySelectorAll(`input.hn-radio[data-param="${name}"]`);

/** The replies of a page whose first snapshot holds `item` and whose next snapshot answers the page's latest wait
 * when `next` is called. */
function replies(item, extra = {}) {
  let next = null;
  return {
    next: (snapshot) => next({ ...snapshot, seq: setup.seq + 1 }),
    replies: {
      agents_snapshot: { ...setup, items: [item] },
      agents_next: () =>
        new Promise((resolve) => {
          next = resolve;
        }),
      agents_setup_typed: { ok: "yes" },
      agents_answer: { ok: "yes" },
      ...extra,
    },
  };
}

const QUESTIONS = Object.keys(HINTS);
const chooseOf = (page, name) => page.document.querySelector(`button[data-act="choose_file"][data-param="${name}"]`);

/** Types `text` into the line of `name` as a person does, in place of what it held. */
async function typeLine(page, name, text) {
  const line = lineOf(page, name);
  line.focus();
  line.value = text;
  await line.dispatchEvent(makeEvent("input"));
  await page.settle();
}

/** A stand-in of the core for the four questions of the spreadsheet agent's set-up item `item`, by the core's rules
 * (crates/halvo-coord/src/items/setup.rs, `setup_step` and `setup_step_more`). It lives on behind a page that is
 * loaded anew, and it keeps the lines that it took. A line of a path question is taken where it starts with a slash, a
 * line of Which cells where it names cells, a line of Which sheet as typed, or, while a list stands, where the list
 * has it; a line that it does not take leaves its question with no line and, for a typed question, under its sentence.
 * With `names`, a line of Which spreadsheet file that it takes and did not hold makes them Which sheet's list and drops
 * the sheet that it held. It makes the ONE body, with the line that it holds of each question, at a call whose `more`
 * is not `yes` once it holds a line of every question; the item then leaves the read, and every later call answers
 * `stale`. It publishes a read with a new seq where the item's read changed, as the core does. `refuse(param)` makes
 * each later call of that question answer `no_service`, with nothing changed. */
function coreOver(item, names = null) {
  const held = {};
  let now = item;
  let seq = setup.seq;
  let body = null;
  let refused = null;
  let waiting = [];
  const snapshot = () => ({ ...setup, seq, items: now ? [now] : [] });
  const publish = (next) => {
    if (JSON.stringify(next) === JSON.stringify(now)) return;
    now = next;
    seq += 1;
    waiting.forEach((resolve) => resolve(snapshot()));
    waiting = [];
  };
  const takes = (param, text, listed) => {
    if (listed.length > 0) return listed.includes(text);
    if (FILES.includes(param)) return text.startsWith("/");
    return param !== "range" || /^[A-Z]+[1-9][0-9]*(:[A-Z]+[1-9][0-9]*)?$/.test(text);
  };
  const sentence = (param) => words[param === "range" ? "param_typed.not_cells" : "param_typed.not_path_mac"];
  return {
    saved: () => body,
    snapshot,
    next: (args) => (args.after !== seq ? snapshot() : new Promise((resolve) => waiting.push(resolve))),
    refuse(param) {
      refused = param;
    },
    typed(args) {
      if (body) return { error: "stale" };
      if (args.param === refused) return { error: "no_service" };
      const asked = now.params.find((p) => p.name === args.param);
      const listed = asked.candidates.map((c) => c.value[0]);
      const taken = takes(args.param, args.text, listed);
      const fresh = taken && held[args.param] !== args.text;
      if (taken) held[args.param] = args.text;
      else delete held[args.param];
      let params = now.params.map((p) => {
        if (p.name !== args.param || listed.length > 0) return p;
        const { not_taken: _, ...plain } = p;
        return taken ? plain : { ...plain, not_taken: sentence(p.name) };
      });
      if (args.param === "workbook" && fresh && names) {
        delete held.sheet;
        params = params.map((p) => (p.name === "sheet" ? listEntry(names) : p));
      }
      const full = QUESTIONS.every((name) => typeof held[name] === "string");
      if (full && args.more !== "yes") body = Object.fromEntries(QUESTIONS.map((name) => [name, held[name]]));
      publish(body ? null : { ...now, params });
      return { ok: "yes" };
    },
  };
}

/** The options of a page over the stand-in `core`; a pick in a chooser answers `picked`. */
function over(core, picked = null) {
  return {
    replies: {
      agents_snapshot: () => core.snapshot(),
      agents_next: (args) => core.next(args),
      agents_setup_typed: (args) => core.typed(args),
      agents_answer: { ok: "yes" },
      agents_choose_file: picked === null ? { closed: "yes" } : { path: picked },
    },
  };
}

/** What the card shows as the answer of each question: the text of its line, or the name chosen in its list; an empty
 * text where it shows no answer. */
function shownOn(page) {
  return Object.fromEntries(
    QUESTIONS.map((name) => {
      const line = lineOf(page, name);
      if (line) return [name, line.value];
      const chosen = radiosOf(page, name).filter((r) => r.checked)[0];
      return [name, chosen ? chosen.parentNode.textContent : ""];
    }),
  );
}

test("a set-up item whose four questions carry stored lines draws each line filled with its stored line, as text, under its hint; one line turned and Use these answers make the calls with the three stored lines and the new one; where the workbook lacks the sheet's name the list stands with its sentence under it at that press; an item whose questions carry no stored line draws empty lines", async () => {
  const one = replies(storedItem);
  const page = await openPage(one);
  for (const name of Object.keys(HINTS)) {
    assert.equal(lineOf(page, name).value, STORED[name], `${name}: the stored line fills it`);
    assert.deepEqual(
      setOf(page, name).querySelectorAll("p.hn-hint").map((p) => p.textContent),
      [words["param_hint." + HINTS[name]]],
      `${name}: the hint stays under the question's words`,
    );
  }
  assert.equal(setOf(page, "workbook").querySelectorAll("b").length, 0, "the stored line is text, never markup");

  const turned = "/Volumes/work/receipts-december.csv";
  const line = lineOf(page, "source");
  line.focus();
  line.value = turned;
  await line.dispatchEvent(makeEvent("input"));
  await page.settle();
  await page.click(useAnswers(page), { detail: 0 });
  assert.deepEqual(
    page.calls("agents_setup_typed").map((c) => [c.args.param, c.args.text]),
    [
      ["workbook", STORED.workbook],
      ["sheet", STORED.sheet],
      ["range", STORED.range],
      ["source", turned],
    ],
    "the three stored lines and the new one",
  );
  assert.equal(page.calls("agents_answer").length, 0, "no other call");
  const lacking = ["Plan", "Actual"];
  one.next({ ...setup, items: [{ ...storedItem, params: storedItem.params.map((p) => (p.name === "sheet" ? listEntry(lacking) : p)) }] });
  await page.settle();
  assert.deepEqual(
    radiosOf(page, "sheet").map((r) => r.parentNode.textContent),
    lacking,
    "the list of a workbook that lacks the sheet's name stands",
  );
  assert.deepEqual(
    setOf(page, "sheet").querySelectorAll("p.hn-full").map((p) => p.textContent),
    [words["param_typed.needs_answer"]],
    "with its sentence under it at that press",
  );

  const empty = await openPage(replies(emptyItem));
  assert.deepEqual(
    Object.keys(HINTS).map((name) => lineOf(empty, name).value),
    ["", "", "", ""],
    "no stored line: every line empty",
  );
});

test("at Change its answers a line that the person empties is an empty line again: with Which sheet emptied, a pick in the chooser of Which spreadsheet file sends its line at once, as a call that says that more follow, and the list of that workbook's sheets comes", async () => {
  const picked = "/Volumes/work/Budget-2027.xlsx";
  const one = replies(storedItem, { agents_choose_file: { path: picked } });
  const page = await openPage(one);
  await typeLine(page, "sheet", "");
  await page.click(chooseOf(page, "workbook"), { detail: 0 });
  assert.deepEqual(
    page.calls("agents_setup_typed").map((c) => c.args),
    [{ item: storedItem.item, step: storedItem.step, param: "workbook", text: picked, more: "yes" }],
    "the picked line goes at the pick",
  );
  const names = ["January", "February"];
  one.next({ ...setup, items: [{ ...storedItem, params: storedItem.params.map((p) => (p.name === "sheet" ? listEntry(names) : p)) }] });
  await page.settle();
  assert.deepEqual(
    radiosOf(page, "sheet").map((r) => r.parentNode.textContent),
    names,
    "the list of the picked workbook's sheets comes",
  );
});

test("what a press of Use these answers or a pick in a chooser saves is what the card shows, or nothing is saved: behind a press that left the set-up open and a page that is then loaded anew, where the core still holds the lines of that press, a question that shows no answer keeps every save back, an empty line and a list with none chosen alike, a line typed anew is saved as typed, and a pick saves nothing; a second press does nothing while a press of the same item is under way, and a press works again behind a chain that ended in a refusal; behind a call that the core refuses no call of that press may save; a line emptied and typed again, and a line typed again behind a press that went out with it empty, are saved as typed", async () => {
  const w2 = "/Volumes/work/Budget-2028.xlsx";
  const d1 = "/Volumes/work/receipts-november.csv";
  const d2 = "/Volumes/work/receipts-december.csv";
  const noPath = "Budget.xlsx";
  const clauses = [];
  let shown = null;
  /** A press of Use these answers, with what the card shows at that press kept in `shown`. */
  const press = async (page) => {
    shown = shownOn(page);
    await page.click(useAnswers(page), { detail: 0 });
  };
  const said = (row, saved) => console.log(`u330 ${row}: SAVED ${JSON.stringify(saved)} | THE CARD SHOWS ${JSON.stringify(shown)}`);
  /** One state: the card of `item` over a stand-in of the core; `earlier` is a press that leaves the set-up open, behind
   * which the page is loaded anew over the same core; `act` is what the person does then, and it ends in a press or in
   * a pick; what is saved stands beside what the card shows at that press or pick, and `saves` says which of the two
   * the rule asks for. */
  async function state(row, { item, names = null, picked = null, earlier = null, act, saves }) {
    const core = coreOver(item, names);
    let page = await openPage(over(core, picked));
    if (earlier) {
      await earlier(page);
      page = await page.reopen();
    }
    await act(page);
    const saved = core.saved();
    said(row, saved);
    clauses.push({ row, broken: JSON.stringify(saved) !== JSON.stringify(saves === "the card" ? shown : null) });
  }

  await state("row 11, an empty line of a question whose line the core holds", {
    item: { ...base, params: [entry("workbook", false), entry("sheet", true), entry("range", true), entry("source", true)] },
    earlier: async (page) => {
      await typeLine(page, "workbook", "/Volumes/work/Budget-2027.xlsx");
      await typeLine(page, "range", "zzz");
      await press(page);
    },
    act: press,
    saves: "nothing",
  });

  const sourceDropped = { ...base, params: [entry("workbook", true), entry("sheet", true), entry("range", true), entry("source", false)] };
  const workbookNotTaken = async (page) => {
    await typeLine(page, "workbook", noPath);
    await typeLine(page, "source", d1);
    await press(page);
  };
  await state("row 12, a line typed anew behind a line that the core lacks", {
    item: sourceDropped,
    earlier: workbookNotTaken,
    act: async (page) => {
      await typeLine(page, "workbook", w2);
      await typeLine(page, "source", d2);
      await press(page);
    },
    saves: "the card",
  });

  await state("row 13, a pick of a workbook while the core holds every other line", {
    item: sourceDropped,
    picked: w2,
    earlier: workbookNotTaken,
    act: async (page) => {
      await typeLine(page, "range", "B2:D12");
      shown = { ...shownOn(page), workbook: w2 };
      await page.click(chooseOf(page, "workbook"), { detail: 0 });
    },
    saves: "nothing",
  });

  await state("row 19, a first set-up whose three taken lines show empty", {
    item: emptyItem,
    earlier: async (page) => {
      await typeLine(page, "workbook", noPath);
      await typeLine(page, "sheet", STORED.sheet);
      await typeLine(page, "range", STORED.range);
      await typeLine(page, "source", d1);
      await press(page);
    },
    act: async (page) => {
      await typeLine(page, "workbook", w2);
      await press(page);
    },
    saves: "nothing",
  });

  await state("row 20, a list with none chosen whose sheet the core holds", {
    item: storedItem,
    names: ["Notes", STORED.sheet],
    earlier: async (page) => {
      await typeLine(page, "range", "zzz");
      await press(page);
    },
    act: async (page) => {
      await typeLine(page, "range", "B2:D12");
      await press(page);
    },
    saves: "nothing",
  });

  {
    const row = "row 21, a second press while a press of the same item is under way";
    const core = coreOver(storedItem);
    let next = null;
    let refuseFirst = null;
    const page = await openPage({
      replies: {
        agents_snapshot: { ...setup, items: [storedItem] },
        agents_next: () =>
          new Promise((resolve) => {
            next = resolve;
          }),
        agents_setup_typed: (args) => {
          if (refuseFirst !== null) return core.typed(args);
          return new Promise((resolve) => {
            refuseFirst = () => resolve({ error: "no_service" });
          });
        },
        agents_answer: { ok: "yes" },
      },
    });
    await typeLine(page, "source", d1);
    await press(page);
    next({ ...setup, seq: setup.seq + 1, items: [storedItem] });
    await page.settle();
    await typeLine(page, "source", d2);
    await press(page);
    const under = page.calls("agents_setup_typed").length;
    refuseFirst();
    await page.settle();
    await press(page);
    const behind = page.calls("agents_setup_typed").length - under;
    const saved = core.saved();
    console.log(`u330 ${row}: CALLS WHILE THE FIRST PRESS IS UNDER WAY ${under} (1 is its own first call) | CALLS OF A PRESS BEHIND THE REFUSED CHAIN ${behind} | SAVED ${JSON.stringify(saved)} | THE CARD SHOWS ${JSON.stringify(shown)}`);
    clauses.push({ row, broken: under !== 1 || behind !== QUESTIONS.length || JSON.stringify(saved) !== JSON.stringify(shown) });
  }

  {
    const row = "a call of the press that the core refuses";
    const core = coreOver(storedItem);
    const page = await openPage(over(core));
    await typeLine(page, "workbook", noPath);
    await press(page);
    const before = page.calls("agents_setup_typed").length;
    await typeLine(page, "workbook", w2);
    await typeLine(page, "range", "B2:D12");
    core.refuse("range");
    await press(page);
    const sent = page.calls("agents_setup_typed").slice(before).map((c) => [c.args.param, c.args.more]);
    const saved = core.saved();
    console.log(`u330 ${row}: CALLS ${JSON.stringify(sent)} | SAVED ${JSON.stringify(saved)} | THE CARD SHOWS ${JSON.stringify(shown)}`);
    const last = sent[sent.length - 1] || [];
    clauses.push({ row, broken: saved !== null || last[0] !== "range" || sent.some((call) => call[1] === "no") });
  }

  await state("STATE 1, a line emptied and typed again behind a page loaded anew, and the lacking line mended", {
    item: storedItem,
    earlier: async (page) => {
      await typeLine(page, "workbook", noPath);
      await press(page);
    },
    act: async (page) => {
      await typeLine(page, "range", "");
      await typeLine(page, "range", "B2:D12");
      await typeLine(page, "workbook", w2);
      await press(page);
    },
    saves: "the card",
  });

  await state("STATE 2, a press with a line emptied, then that line typed again and another changed", {
    item: storedItem,
    act: async (page) => {
      await typeLine(page, "workbook", "");
      await press(page);
      await typeLine(page, "workbook", w2);
      await typeLine(page, "range", "B2:D12");
      await press(page);
    },
    saves: "the card",
  });

  assert.deepEqual(
    clauses.filter((c) => c.broken).map((c) => c.row),
    [],
    "the states in which another text is saved than the card shows, a text is saved while a question shows no answer, a call goes while a press is under way, or a call that may save goes behind a refused one",
  );
});

/** The sentences that stand under the question `name`. */
const under = (page, name) => setOf(page, name).querySelectorAll("p.hn-full").map((p) => p.textContent);
/** The list of Which sheet: the name of each entry, and whether it is chosen. */
const listOf = (page) => radiosOf(page, "sheet").map((r) => [r.parentNode.textContent, r.checked === true]);

test("at Change its answers the sentence under an emptied line of Which cells and the focus on it stay when the workbook's names are read at the press and Which sheet turns into its list: the line is not drawn anew, and nothing is saved", async () => {
  const core = coreOver(storedItem, ["Notes", STORED.sheet]);
  const page = await openPage(over(core));
  await typeLine(page, "range", "");
  const line = lineOf(page, "range");
  await page.click(useAnswers(page), { detail: 0 });
  assert.deepEqual(
    page.calls("agents_setup_typed").map((c) => [c.args.param, c.args.text, c.args.more]),
    [
      ["workbook", STORED.workbook, "yes"],
      ["sheet", STORED.sheet, "yes"],
      ["source", STORED.source, "yes"],
    ],
    "the three other lines go, each as a call that says that more follow",
  );
  assert.deepEqual(
    listOf(page),
    [
      ["Notes", false],
      [STORED.sheet, true],
    ],
    "the names were read at that press: Which sheet is its list, with the sheet of its line chosen",
  );
  assert.deepEqual(under(page, "range"), [words["param_typed.needs_answer"]], "the sentence stands under Which cells behind the newer read");
  assert.ok(page.document.activeElement === lineOf(page, "range"), "the focus is on the card's line of Which cells");
  assert.ok(lineOf(page, "range") === line, "and that line is the same node as before the press");
  assert.equal(core.saved(), null, "nothing is saved");
});

test("a question that is drawn anew at the same step stands under the sentence where the old card showed it there and the question still shows no answer: with Which sheet emptied at the press, the list of the workbook's sheets comes with none chosen, under the sentence, and the focus goes from the line to the card's heading and to no control; and where the list lacks the sheet of the press, the sentence under it still stands behind a second newer read of that one press", async () => {
  const sentence = words["param_typed.needs_answer"];
  const readings = [];
  /** One reading: what the page shows under `what`, printed, beside what the rule asks for. */
  const read = (what, got, want) => {
    console.log(`u331 ${what}: ${JSON.stringify(got)}`);
    readings.push({ what, broken: JSON.stringify(got) !== JSON.stringify(want) });
  };

  const core = coreOver(storedItem, ["Notes", STORED.sheet]);
  const page = await openPage(over(core));
  await typeLine(page, "sheet", "");
  await page.click(useAnswers(page), { detail: 0 });
  read("a line that turned into its list, the list", listOf(page), [
    ["Notes", false],
    [STORED.sheet, false],
  ]);
  read("a line that turned into its list, the sentences under it", under(page, "sheet"), [sentence]);
  const heading = page.document.querySelector("h3.hn-title");
  read(
    "a line that turned into its list, the focus is on the card's heading, whose tab index is -1, and on no entry of the list",
    [page.document.activeElement === heading, heading.getAttribute("tabindex"), radiosOf(page, "sheet").includes(page.document.activeElement)],
    [true, "-1", false],
  );
  read("a line that turned into its list, saved", core.saved(), null);

  const lacking = coreOver(storedItem, ["Plan", "Actual"]);
  let answerLast = null;
  const lastHeld = new Promise((resolve) => {
    answerLast = resolve;
  });
  const options = over(lacking);
  const twice = await openPage({
    replies: { ...options.replies, agents_setup_typed: (args) => (args.param === "source" ? lastHeld.then(() => lacking.typed(args)) : lacking.typed(args)) },
  });
  await typeLine(twice, "source", "receipts.csv");
  await twice.click(useAnswers(twice), { detail: 0 });
  read("one press, behind its first newer read (the list has come), the list", listOf(twice), [
    ["Plan", false],
    ["Actual", false],
  ]);
  read("one press, behind its first newer read, the sentences under the list", under(twice, "sheet"), [sentence]);
  read("one press, behind its first newer read, the sentences under the data file's line", under(twice, "source"), []);
  answerLast();
  await twice.settle();
  read("one press, behind its second newer read, the sentences under the data file's line", under(twice, "source"), [words["param_typed.not_path_mac"]]);
  read("one press, behind its second newer read, the list", listOf(twice), [
    ["Plan", false],
    ["Actual", false],
  ]);
  read("one press, behind its second newer read, the sentences under the list", under(twice, "sheet"), [sentence]);
  read("one press, behind its second newer read, saved", lacking.saved(), null);

  assert.deepEqual(
    readings.filter((r) => r.broken).map((r) => r.what),
    [],
    "the readings in which the page shows another thing than the rule asks for",
  );
});
