import { describe, expect, test } from "vitest";
import { type Asked, driven } from "./u56p2-provider-world.ts";

// Round three (ruling P7-120 with its Addition 1, N1 to N4): the stored id's three states through the real adapter in the
// real object. An unknown answer for the stored id counts only in a call whose read of the customer showed the key right;
// the first that counts is kept and answers failed, the second, in a later try, settles the id and writes one line.

const stored = (asked: Asked[], id: string) => asked.filter((each) => each.path === `/v1/subscriptions/${id}`).map((each) => `${each.attempt} ${each.method}`);
const DAY_TRIES = { delayMs: 100 };

describe("N3: a stored id that sits on another customer", () => {
  test("H3a: tries 1 and 2 under a key of the other mode (the stored id and the customer both 404) count for nothing; try 3 under the right key asks the stored id again and cancels it", async () => {
    const { ending, w } = await driven({ plans: 0, noFirstPlan: false, stored: true, storedElsewhere: true, ...DAY_TRIES, wrongKey: (attempt) => attempt <= 2 });
    w.close();
    expect({
      answers: ending.answers,
      storedRequests: stored(ending.asked, "sub_1"),
      columnsAfter: ending.columnsAfter.slice(0, 2),
      live: ending.live,
      gone: ending.goneAfterMs !== null,
      lines: ending.lines["cancel-stored-unknown"] ?? 0,
    }).toEqual({
      answers: ["failed 0/0", "failed 0/0", "cancelled"],
      storedRequests: ["1 DELETE", "1 GET", "2 DELETE", "2 GET", "3 DELETE"],
      columnsAfter: [
        [null, null],
        [null, null],
      ],
      live: [],
      gone: true,
      lines: 0,
    });
  });

  test("H3c: the right key, and the provider answers the stored id as unknown at try 1 alone: try 1 keeps the first unknown and answers failed, try 2 asks it again, cancels it (an ended answer after a kept first unknown) and writes no line", async () => {
    const { ending, w } = await driven({ plans: 0, stored: true, storedElsewhere: true, ...DAY_TRIES, storedUnknownAt: (attempt) => attempt === 1 });
    w.close();
    expect({
      answers: ending.answers,
      storedRequests: stored(ending.asked, "sub_1"),
      columnsAfter: ending.columnsAfter,
      waits: ending.waits,
      live: ending.live,
      lines: ending.lines["cancel-stored-unknown"] ?? 0,
    }).toEqual({
      answers: ["failed 0/0", "cancelled"],
      storedRequests: ["1 DELETE", "1 GET", "2 DELETE"],
      columnsAfter: [[null, "sub_1"]],
      waits: [60_000],
      live: [],
      lines: 0,
    });
  });
});

describe("N2's pace (the coordinator's reading 2): a newly kept first unknown sets the next try at 60 s with the delay back to 60 s", () => {
  test("tries 1 and 2 under a key of the other mode double the pause; try 3 keeps the first unknown and the next try comes 60 s later, where it asks the stored id again and cancels it", async () => {
    const { ending, w } = await driven({
      plans: 0,
      stored: true,
      storedElsewhere: true,
      ...DAY_TRIES,
      wrongKey: (attempt) => attempt <= 2,
      storedUnknownAt: (attempt) => attempt === 3,
    });
    w.close();
    expect({ answers: ending.answers, waits: ending.waits, columnsAfter: ending.columnsAfter, live: ending.live }).toEqual({
      answers: ["failed 0/0", "failed 0/0", "failed 0/0", "cancelled"],
      waits: [60_000, 120_000, 60_000],
      columnsAfter: [
        [null, null],
        [null, null],
        [null, "sub_1"],
      ],
      live: [],
    });
  });
});

describe("N3: an id that the provider truly does not know", () => {
  test("failed at try 1 with the first unknown kept, settled at try 2 with one cancel-stored-unknown line, and no request for it at try 3", async () => {
    const { ending, w } = await driven({
      plans: 0,
      noFirstPlan: true,
      stored: "sub_gone",
      ...DAY_TRIES,
      beforeList: (attempt, n, made) => {
        if (attempt === 2 && n === 1) made("sub_late_a");
        if (attempt === 2 && n === 2) made("sub_late_b");
      },
    });
    w.close();
    expect({
      answers: ending.answers,
      storedRequests: stored(ending.asked, "sub_gone"),
      columnsAfter: ending.columnsAfter,
      lines: ending.lines["cancel-stored-unknown"] ?? 0,
      live: ending.live,
      gone: ending.goneAfterMs !== null,
    }).toEqual({
      answers: ["failed 0/0", "failed 1/0", "cancelled"],
      storedRequests: ["1 DELETE", "1 GET", "2 DELETE", "2 GET"],
      columnsAfter: [
        [null, "sub_gone"],
        ["sub_gone", null],
      ],
      lines: 1,
      live: [],
      gone: true,
    });
  });

  test("a customer read that answers 500 at try 1 counts nothing: nothing is kept after it, try 2 keeps the first unknown, try 3 settles it and ends", async () => {
    const { ending, w } = await driven({ plans: 0, noFirstPlan: true, stored: "sub_gone", ...DAY_TRIES, customerRead: (attempt) => (attempt === 1 ? "500" : undefined) });
    w.close();
    expect({ answers: ending.answers, columnsAfter: ending.columnsAfter, lines: ending.lines["cancel-stored-unknown"] ?? 0 }).toEqual({
      answers: ["failed 0/0", "failed 0/0", "nothing-to-cancel"],
      columnsAfter: [
        [null, null],
        [null, "sub_gone"],
      ],
      lines: 1,
    });
  });

  test("a customer read cut by the deadline while a first unknown is kept keeps it: it neither counts nor starts the count again", async () => {
    const { ending, w } = await driven({ plans: 0, noFirstPlan: true, stored: "sub_gone", ...DAY_TRIES, customerRead: (attempt) => (attempt === 2 ? "cut" : undefined) });
    w.close();
    expect({ answers: ending.answers, columnsAfter: ending.columnsAfter, lines: ending.lines["cancel-stored-unknown"] ?? 0 }).toEqual({
      answers: ["failed 0/0", "failed 0/0", "nothing-to-cancel"],
      columnsAfter: [
        [null, "sub_gone"],
        [null, "sub_gone"],
      ],
      lines: 1,
    });
  });

  test("a deleted customer with an unknown stored id: failed at try 1, then nothing-to-cancel at try 2 with the line", async () => {
    const { ending, w } = await driven({ plans: 0, noFirstPlan: true, stored: "sub_gone", ...DAY_TRIES, customerRead: () => "deleted" });
    w.close();
    expect({ answers: ending.answers, gone: ending.goneAfterMs !== null, lines: ending.lines["cancel-stored-unknown"] ?? 0 }).toEqual({
      answers: ["failed 0/0", "nothing-to-cancel"],
      gone: true,
      lines: 1,
    });
  });
});

describe("N3: the count starts again (the coordinator's reading of 2026-11-16 of N3's last test)", () => {
  test("a refusal read as live between two counted unknown answers starts the count again: try 3 is a first unknown again (failed, no line), and the cancel at try 4 ends it", async () => {
    const { ending, w } = await driven({
      plans: 0,
      stored: true,
      storedElsewhere: true,
      ...DAY_TRIES,
      storedUnknownAt: (attempt) => attempt === 1 || attempt === 3,
      storedRefusedAt: (attempt) => attempt === 2,
    });
    w.close();
    expect({
      answers: ending.answers,
      columnsAfter: ending.columnsAfter,
      storedRequests: stored(ending.asked, "sub_1"),
      lines: ending.lines["cancel-stored-unknown"] ?? 0,
      live: ending.live,
    }).toEqual({
      answers: ["failed 0/0", "failed 0/1", "failed 0/0", "cancelled"],
      columnsAfter: [
        [null, "sub_1"],
        [null, null],
        [null, "sub_1"],
      ],
      storedRequests: ["1 DELETE", "1 GET", "2 DELETE", "2 GET", "3 DELETE", "3 GET", "4 DELETE"],
      lines: 0,
      live: [],
    });
  });
});

describe("N4: an account with a stored id and no customer id (a named limit, reachable through the test seed alone)", () => {
  test("an unknown answer never counts there: the stored id is asked again on every try and cancelled when it can be, and the delete stays open", async () => {
    const { ending, w } = await driven({ plans: 0, stored: true, customer: false, ...DAY_TRIES, wrongKey: (attempt) => attempt <= 2, days: 1 });
    w.close();
    expect({
      storedRequests: stored(ending.asked, "sub_1"),
      columnsAfter: ending.columnsAfter.slice(0, 3),
      otherRequests: ending.asked.filter((each) => !each.path.startsWith("/v1/subscriptions/sub_1")).length,
      first: ending.answers.slice(0, 4),
      gone: ending.goneAfterMs,
      live: ending.live,
    }).toEqual({
      storedRequests: ["1 DELETE", "1 GET", "2 DELETE", "2 GET", "3 DELETE"],
      columnsAfter: [
        [null, null],
        [null, null],
        ["sub_1", null],
      ],
      otherRequests: 0,
      first: ["failed 0/0", "failed 0/0", "failed 0/0", "failed 0/0"],
      gone: null,
      live: [],
    });
  });
});
