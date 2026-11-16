import { describe, expect, test } from "vitest";
import { driven, runs } from "./u56p2-provider-world.ts";

const HOUR = 3_600_000;

describe("X2: once the stored id is settled every try whose first listed cancel is answered ends one, up to 3,333 ms a request (P7-119 Addition 1)", () => {
  test("2,400 ms a request, a stored id, a customer of 2: it ends in 2 tries", async () => {
    const { ending, w } = await driven({ plans: 1, stored: true, delayMs: 2400 });
    w.close();
    expect({ tries: ending.tries, answers: ending.answers, gone: ending.goneAfterMs !== null, live: ending.live }).toEqual({
      tries: 2,
      answers: ["failed 1/0", "nothing-to-cancel"],
      gone: true,
      live: [],
    });
  });

  test("2,400 ms a request, a stored id, a customer of 250: it ends in 126 tries, 2 listed cancels a try once the stored id is settled", async () => {
    const { ending, w } = await driven({ plans: 249, stored: true, delayMs: 2400 });
    w.close();
    expect({ tries: ending.tries, runs: runs(ending.answers), gone: ending.goneAfterMs !== null, live: ending.live.length, periods: ending.periodsBegunAfterMark }).toEqual({
      tries: 126,
      runs: ["failed 1/0 x1", "failed 2/0 x124", "nothing-to-cancel x1"],
      gone: true,
      live: 0,
      periods: 0,
    });
  });

  test("3,000 ms a request, a stored id, a customer of 2: it ends in 3 tries", async () => {
    const { ending, w } = await driven({ plans: 1, stored: true, delayMs: 3000 });
    w.close();
    expect({ answers: ending.answers, gone: ending.goneAfterMs !== null, live: ending.live }).toEqual({
      answers: ["failed 0/1", "failed 1/0", "nothing-to-cancel"],
      gone: true,
      live: [],
    });
  });

  test("3,000 ms a request, a stored id, a customer of 250: it ends in 251 tries, 1 listed cancel a try once the stored id is settled, and one period begins during the 4.2 hours", async () => {
    const { ending, w } = await driven({ plans: 249, stored: true, delayMs: 3000 });
    w.close();
    expect({ tries: ending.tries, runs: runs(ending.answers), gone: ending.goneAfterMs !== null, live: ending.live.length, periods: ending.periodsBegunAfterMark }).toEqual({
      tries: 251,
      runs: ["failed 0/1 x1", "failed 1/1 x248", "failed 1/0 x1", "nothing-to-cancel x1"],
      gone: true,
      live: 0,
      periods: 1,
    });
  });

  test("the edge with the stored id settled or absent: 3,333 ms a request still ends a customer of 250 with no stored id", async () => {
    const { ending, w } = await driven({ plans: 250, stored: false, delayMs: 3333 });
    w.close();
    expect({ gone: ending.goneAfterMs !== null, live: ending.live.length }).toEqual({ gone: true, live: 0 });
  });

  test("the named limit above 3,333 ms: at 3,400 ms a request a customer of 2 never ends in 7 days; each try is failed with its second count, the pause doubles to the cap and cancel-unanswered is written at most once an hour", async () => {
    const { ending, w } = await driven({ plans: 1, stored: true, delayMs: 3400, days: 7 });
    w.close();
    const lines = ending.lines["cancel-unanswered"] ?? 0;
    expect({
      gone: ending.goneAfterMs,
      live: ending.live,
      firstAnswers: ending.answers.slice(0, 3),
      capped: ending.waits.slice(-1),
      linesAtMostHourly: lines >= 1 && lines <= 7 * 24 + 1,
    }).toEqual({ gone: null, live: ["sub_00002"], firstAnswers: ["failed 0/0", "failed 0/1", "failed 0/1"], capped: [HOUR], linesAtMostHourly: true });
  });
});
