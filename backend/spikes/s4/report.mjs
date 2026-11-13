import { readFile, writeFile } from 'node:fs/promises';
import { countFor, pairCounts } from '../../test-kit/checkout-page/fields.mjs';
import { loadFields } from '../../test-kit/checkout-page/server.mjs';
import {
  alteredBudgets,
  BUDGET_PATH_ID,
  budgetRowFaults,
  budgetRows,
  checkoutActions,
  classCount,
  EXPECTED,
  logEqualsPage,
  rank,
  reachedActiveWithCap,
  typedCount,
} from './rule.mjs';

const ROOT = new URL('../../', import.meta.url);
const RESULT = new URL('RESULT.md', import.meta.url);
const BUDGET = new URL('gate/prompt-budget.json', ROOT);
const MARKER = '\n## Stated before the third run\n';
const FIRST_RUN =
  'The first run, whose app page had a second click, read MISSED at 10 and 9 actions against the 8 and 7 stated before it';
const EARLIER_RUNS = `The second run read MET at the same counts, with a browser that the driver started itself inside the sandbox. ${FIRST_RUN}`;
const LABEL = {
  device: 'device credential, no sign-in step',
  passkey: 'passkey first',
  google: 'Google-style sign-in',
};
const BUYER = { us: 'US', gb: 'UK' };
const PROMPT_NO_CHECK_HERE_CAN_RAISE = {
  passkey:
    "the system's passkey prompt (Touch ID, Windows Hello or a PIN), at least one on real hardware (researcher-5, option 3)",
  google:
    "the identity provider's own page, which asks for one or two confirmations in a browser that is already signed in (researcher-2 summary, an inference with no primary source) and for more in any other",
};

function stop(message) {
  console.error(`s4: ${message}; nothing was written (see .tmp/s4/sandbox.txt)`);
  process.exit(2);
}

const denied = process.argv[2] ?? 'uncounted';
const measured = await readFile(new URL('.tmp/s4/measured.json', ROOT), 'utf8')
  .then(JSON.parse)
  .catch(() => stop('the measuring step wrote no result that can be read'));
if (process.env.S4_RUN_ID && measured.runId !== process.env.S4_RUN_ID) {
  stop('the measuring step did not finish, so its result file is from an earlier run');
}
const previous = await readFile(RESULT, 'utf8');
if (!previous.includes(MARKER)) stop('spikes/s4/RESULT.md has lost the heading "Stated before the third run"');
const stated = previous.slice(previous.indexOf(MARKER));
const statedCount = (form) => Number(new RegExp(`^\\| ${form} \\| 1 \\| \\d \\| 1 \\| (\\d+) \\|$`, 'm').exec(stated)?.[1]);
if (
  statedCount('a card with a postal code \\(US buyer\\)') !== EXPECTED['device-us'] ||
  statedCount('a card without one \\(UK buyer\\)') !== EXPECTED['device-gb']
) {
  stop('the expected counts in rule.mjs are not the ones that RESULT.md states before the second run');
}

const fields = await loadFields();
const path = Object.fromEntries(measured.paths.map((record) => [record.id, record]));
const control = Object.fromEntries(measured.controls.map((record) => [record.id, record]));
const us = path[BUDGET_PATH_ID];
const gb = path['device-gb'];
const deviceRuns = [us, gb];
const plural = (count, word) => `${count} ${word}${count === 1 ? '' : 's'}`;
const list = (items) => (items.length > 1 ? `${items.slice(0, -1).join(', ')} and ${items.at(-1)}` : items.join(''));
const sourceText = (source) =>
  source.type === 'docs' ? `docs: ${source.rows.join(', ')}` : source.type === 'plan' ? `plan: ${source.ref}` : 'unknown';
const reachedText = (record) => {
  if (reachedActiveWithCap(record)) return 'active with a cap';
  if (record.error) return `not reached: ${record.error}`;
  return record.reached ? `${record.reached.subscription}, cap ${record.reached.cap ? 'set' : 'not set'}` : 'no account';
};
const name = (record) => `${LABEL[record.candidate]}, ${BUYER[record.buyer]} buyer`;

const budget = {
  about:
    'Written by sh spikes/s4/run.sh. One row for each action that the driver logged on the device-credential path with a US buyer, then the two rows that no check on this machine can observe. budget is the number of measured rows. expected is the count stated before the run (spikes/s4/RESULT.md, Stated before the second run). A row that is not measured carries no count and adds nothing to the budget.',
  path: name(us),
  budget: reachedActiveWithCap(us) ? us.log.length : null,
  expected: EXPECTED[BUDGET_PATH_ID],
  rows: budgetRows(us, fields),
};
await writeFile(BUDGET, `${JSON.stringify(budget, null, 2)}\n`);
const written = JSON.parse(await readFile(BUDGET, 'utf8'));
const faults = budgetRowFaults(written.rows, us);

const altered = alteredBudgets(written.rows).map((copy) => ({ ...copy, faults: budgetRowFaults(copy.rows, us) }));
const others = [...measured.paths, ...measured.controls].filter((record) => record.id !== 'remote-request');
const controls = [
  {
    label: 'unlogged input',
    showed: `the page counted ${plural(control['unlogged-input'].page?.count ?? 0, 'input')} and the log holds ${plural(checkoutActions(control['unlogged-input']), 'action')} for the page`,
    failed:
      !logEqualsPage(control['unlogged-input']) &&
      control['unlogged-input'].page?.count === checkoutActions(control['unlogged-input']) + 1,
  },
  {
    label: 'no payment',
    showed: `the page answered "${control['no-payment'].checkoutStatus}" and the backend reports: ${reachedText(control['no-payment'])}`,
    failed: control['no-payment'].checkoutStatus === 'refused' && control['no-payment'].reached?.subscription === 'pending',
  },
  {
    label: 'budget rows',
    showed: altered.map((copy) => `${copy.id}: ${copy.faults[0] ?? 'no fault found'}`).join('; '),
    failed: altered.every((copy) => copy.faults.length > 0),
  },
  {
    label: 'remote request',
    showed: `the driver counted ${plural(control['remote-request'].outside.length, 'request')} outside loopback in the control and ${others.reduce((sum, record) => sum + record.outside.length, 0)} in the ${others.length} other runs`,
    failed: control['remote-request'].outside.length === 1 && others.every((record) => record.outside.length === 0),
  },
];

const candidates = measured.paths.map((record) => ({
  record,
  count: reachedActiveWithCap(record) ? record.log.length : null,
  reason: reachedActiveWithCap(record) ? null : `not measured: ${reachedText(record)}`,
}));
const overExpected = deviceRuns.filter((record) => record.log.length > EXPECTED[record.id]);
const conditions = [
  {
    holds: deviceRuns.every(reachedActiveWithCap),
    text: 'both device-credential runs reach the state active with a cap set',
    values: deviceRuns.map((record) => `${BUYER[record.buyer]} buyer: ${reachedText(record)}`).join('; '),
  },
  {
    holds: deviceRuns.every(logEqualsPage),
    text: "in both runs the driver's log for the checkout page equals the page's own count",
    values: deviceRuns
      .map((record) => `${BUYER[record.buyer]} buyer: ${checkoutActions(record)} logged, ${record.page?.count ?? 'nothing'} counted by the page`)
      .join('; '),
  },
  {
    holds: deviceRuns.every(reachedActiveWithCap) && overExpected.length === 0,
    text: 'neither device-credential run logs more actions than its expected count',
    values: deviceRuns.map((record) => `${BUYER[record.buyer]} buyer: ${record.log.length} logged, ${EXPECTED[record.id]} expected`).join('; '),
  },
  {
    holds: faults.length === 0,
    text: 'gate/prompt-budget.json holds one row for each action logged on the US run, the two rows that are not measured, and no other row',
    values: faults.length
      ? faults.join('; ')
      : `budget ${written.budget} beside expected ${written.expected}, ${plural(written.rows.filter((row) => row.status === 'measured').length, 'measured row')} for ${plural(us.log.length, 'logged action')}, ${written.rows.filter((row) => row.status === 'not measured').length} rows not measured`,
  },
  {
    holds:
      ['device', 'passkey', 'google'].every((id) => candidates.some((entry) => entry.record.candidate === id)) &&
      candidates.every((entry) => entry.count !== null || entry.reason),
    text: 'this file gives each of the three candidates its count, or the reason it was not measured',
    values: candidates.map((entry) => `${name(entry.record)}: ${entry.count ?? entry.reason}`).join('; '),
  },
  {
    holds: controls.every((entry) => entry.failed),
    text: 'every control fails as stated',
    values: `${controls.filter((entry) => entry.failed).length} of ${controls.length}`,
  },
];
const sandboxText = await readFile(new URL('.tmp/s4/sandbox.txt', ROOT), 'utf8').catch(() => '');
const netLogCounted = /^outbound: browser: net log: \d+ attempts outside loopback in \d+ events, \d+ left out as probes of IPv6$/m.test(sandboxText);
const outboundFaults = [
  ...(measured.sandboxed ? [] : ['the measuring step did not run under the outbound check']),
  ...(measured.browserFromCheck ? [] : ['the driver did not get its browser from the outbound check']),
  ...(/^\d+$/.test(denied) ? [] : ['the outbound check could not count']),
  ...(/^[1-9]\d*$/.test(denied) ? [`the outbound check counted ${denied} denied attempts`] : []),
  ...(netLogCounted ? [] : ["the outbound check printed no count of Chromium's net log"]),
];
const sandboxClean = outboundFaults.length === 0;
const targets = (record) => record.log.filter((action) => action.where === 'checkout page').map((action) => action.target);
const listFaults = deviceRuns
  .filter((record) => record.page && [...record.page.ids].sort().join() !== targets(record).sort().join())
  .map((record) => `the page's own list of inputs differs from the log's for the ${BUYER[record.buyer]} buyer`);
const met = conditions.every((condition) => condition.holds) && sandboxClean && listFaults.length === 0;

const usFields = us.log.filter((action) => action.where === 'checkout page' && action.kind !== 'click');
const payClick = us.log.find((action) => action.where === 'checkout page' && action.kind === 'click');
const beyond = us.log.filter(
  (action) => !(action.where === 'checkout page' && action.kind !== 'click') && action.class !== 'start' && action !== payClick,
);
const beyondText = beyond.map((action) => `action ${action.n}, the ${action.kind} on ${action.target} (${action.where}, class ${action.class})`);
const failing = conditions.flatMap((condition, index) => (condition.holds ? [] : [index + 1]));
const holding = conditions.flatMap((condition, index) => (condition.holds ? [index + 1] : []));
const reasons = failing.map((n) =>
  n === 3 && deviceRuns.every(reachedActiveWithCap)
    ? `condition 3 does not hold: the device-credential path took ${plural(us.log.length, 'action')} with a US buyer against ${EXPECTED[us.id]} expected, and ${gb.log.length} against ${EXPECTED[gb.id]} with a UK buyer${beyond.length && us.log.length - EXPECTED[us.id] === beyond.length ? `; beyond the expected count ${beyond.length === 1 ? 'is' : 'are'} ${list(beyond.map((action) => `the ${action.kind} on ${action.target}`))}` : ''}`
    : `condition ${n} does not hold: ${conditions[n - 1].values}`,
);
if (!sandboxClean || listFaults.length) reasons.push(`the run does not count: ${[...outboundFaults, ...listFaults].join('; ')}`);
const ruleLine = met
  ? `rule: MET (third run: the device-credential path reaches an active subscription with a cap in ${plural(us.log.length, 'action')} for a US buyer and ${gb.log.length} for a UK buyer, against ${EXPECTED[us.id]} and ${EXPECTED[gb.id]} expected, and all six conditions hold. ${EARLIER_RUNS})`
  : `rule: MISSED (third run: ${reasons.join('; ')}${holding.length ? `; ${holding.length === 1 ? 'condition' : 'conditions'} ${list(holding.map(String))} ${holding.length === 1 ? 'holds' : 'hold'}` : ''}. ${EARLIER_RUNS})`;

const ranked = candidates.filter((entry) => entry.record.buyer === 'us' && entry.count !== null).map((entry) => entry.record);
const unranked = candidates.filter((entry) => entry.record.buyer === 'us' && entry.count === null);
const unraisedPrompt = (record) => PROMPT_NO_CHECK_HERE_CAN_RAISE[record.candidate] ?? null;
const rankLines = rank(ranked).map(
  (record, index) =>
    `${index + 1}. ${LABEL[record.candidate]}: ${plural(classCount(record, 'outside'), 'outside action')}, ${plural(typedCount(record), 'typed field')}.${unraisedPrompt(record) ? ` Not measured, and not read as zero: ${unraisedPrompt(record)}.` : ''}`,
);
const withLeast = rank(ranked, (record) => (unraisedPrompt(record) ? 1 : 0)).map(
  (record) => `${LABEL[record.candidate]} (${classCount(record, 'outside') + (unraisedPrompt(record) ? 1 : 0)})`,
);

const signIn = (record) => record.log.filter((action) => action.where === 'browser').length;
const delta = (record) => {
  const typed = typedCount(record) - typedCount(us);
  const changes = [`${plural(signIn(record), 'click')} on the sign-in page more`];
  if (typed !== 0) changes.push(`${plural(Math.abs(typed), 'typed field')} ${typed < 0 ? 'fewer' : 'more'}`);
  return `${plural(record.log.length, 'action')} against the device credential's ${us.log.length}: ${list(changes)}`;
};

const env = measured.environment;
const usCount = countFor(fields, { country: 'US', emailLocked: false });
const gbCount = countFor(fields, { country: 'GB', emailLocked: false });
const dialogs = measured.paths.reduce((sum, record) => sum + record.log.filter((action) => action.kind === 'dialog').length, 0);
const authOf = (record) => record.auth ?? { users: 0, sessions: 0, passkeys: 0, sessionsWithIpOrUserAgent: 0 };
const signInRuns = measured.paths.filter((record) => record.candidate !== 'device');
const sessionRows = signInRuns.reduce((sum, record) => sum + authOf(record).sessions, 0);
const sessionRowsWithIdentity = signInRuns.reduce((sum, record) => sum + authOf(record).sessionsWithIpOrUserAgent, 0);

const lines = [
  ruleLine,
  '',
  '# S4: prompt count through checkout',
  '',
  `Measured on ${env.cpu}, ${env.memoryGiB} GiB, ${env.os}, Node ${env.node}: headless Chromium ${env.chromium} through Playwright ${env.playwright}; Better Auth ${env.betterAuth} with @better-auth/passkey ${env.passkeyPlugin} on node:sqlite, in memory, telemetry off (its option, and BETTER_AUTH_TELEMETRY=${env.telemetrySwitch}); oauth2-mock-server ${env.oauthMock}. Every page and server is a local stand-in on loopback. The commit that holds this file dates the run. This is the third run: the pages, the driver's actions and the rule of the second, with a browser that the repository's outbound check starts. After the measured part come what was stated before this run, then the record of the second run and of the first, each followed by what was stated before it.`,
  '',
  '## Counts',
  '',
  '| candidate | buyer | reached | actions | start | card | cap | outside | typed fields |',
  '|---|---|---|---|---|---|---|---|---|',
  ...candidates.map(
    ({ record, count, reason }) =>
      `| ${LABEL[record.candidate]} | ${BUYER[record.buyer]} | ${reachedText(record)} | ${count ?? reason} | ${classCount(record, 'start')} | ${classCount(record, 'card')} | ${classCount(record, 'cap')} | ${classCount(record, 'outside')} | ${typedCount(record)} |`,
  ),
  '',
  'An action is one entry in the driver\'s log: a click, a typed field or a selection. The classes are the plan\'s: start is the first click, card is the card form on the payment page, cap is the spend cap, outside is everything else. The email typed on the payment page is class outside for every candidate.',
  '',
  `Ranked by the rule on the logged actions, US buyer (fewer outside actions first, fewer typed fields on a tie):`,
  '',
  ...rankLines,
  ...unranked.map((entry) => `- Not ranked: ${LABEL[entry.record.candidate]}, ${entry.reason}.`),
  '',
  `The order above counts only what the driver did. The rows named in it as not measured could not appear on this machine. If each adds one action, the least it can add, the order by outside actions is: ${withLeast.join(', ')}.`,
  '',
  '## What the result decides',
  '',
  `- The budget. The device-credential path took ${plural(us.log.length, 'action')} with a US buyer (${gb.log.length} with a UK buyer, who gets no postal code). The count stated before this run was ${EXPECTED[us.id]} (${EXPECTED[gb.id]} for the UK buyer). The log holds ${plural(classCount(us, 'start'), 'click')} in the app, ${plural(usFields.length, 'checkout field')}${payClick ? ' and the click on the pay button' : ''}${beyond.length ? `, and ${beyond.length} more: ${list(beyondText)}` : ''}. ${classCount(us, 'cap') === 0 ? 'No action is of class cap, because the click in the app sets the cap.' : `${plural(classCount(us, 'cap'), 'action')} of class cap.`} gate/prompt-budget.json records budget ${written.budget} and expected ${written.expected}. ${FIRST_RUN}.`,
  `- D7, first-run identity. The device credential adds no action before the payment page. ${signInRuns
    .filter((record) => reachedActiveWithCap(record))
    .map((record) => `${LABEL[record.candidate][0].toUpperCase()}${LABEL[record.candidate].slice(1)} logs ${delta(record)}. It also carries a row that is not measured.`)
    .join(' ')} A candidate with a sign-in step goes to the owner with its count and is not built on it.`,
  `- D8, the return path after a browser sign-in. The device-credential path opens no sign-in page (${signIn(us)} actions in the browser outside the payment page), so D8 is not on its first-run path. ${
    signInRuns.length ? `Both other candidates pass through the sign-in page, so D8 is on theirs. In this stand-in the sign-in page goes straight on to the payment page and never returns to the app in between.` : ''
  }`,
  '',
  '## The device-credential path, action by action (US buyer)',
  '',
  '| n | where | class | action | target | source | status |',
  '|---|---|---|---|---|---|---|',
  ...written.rows.map((row) =>
    row.status === 'measured'
      ? `| ${row.n} | ${row.where} | ${row.class} | ${row.action} | ${row.target} | ${sourceText(row.source)} | measured |`
      : `| | ${row.where} | ${row.class} | | ${row.label} | ${sourceText(row.source)} | not measured |`,
  ),
  '',
  'These are the rows of gate/prompt-budget.json. The two rows that are not measured carry no count and add nothing to the budget.',
  '',
  '## The conditions of the rule line',
  '',
  ...conditions.map(
    (condition, index) => `${index + 1}. ${condition.holds ? 'Holds' : 'Does not hold'}: ${condition.text} (${condition.values}).`,
  ),
  '',
  '## Outbound',
  '',
  sandboxClean
    ? `This run counts. The measuring step ran under scripts/outbound-hosts.sh browser: the outbound check started the pinned Chromium under its second profile, and the driver connected to it through test-kit/browser. The check counted no denied attempt (denied=0): no record of a process that it ended, an empty Node record, and no attempt outside loopback in Chromium's net log of the whole step. That count leaves out Chromium's probes of IPv6 reachability, a UDP connect to [2001:4860:4860::8888]:443 that sends no packet and that the check denies in any case. The check's output says how many it left out; the number changes from run to run and is in .tmp/s4/sandbox.txt. The driver refused ${measured.paths.reduce((sum, record) => sum + record.outside.length, 0)} page requests outside loopback over the ${measured.paths.length} measured runs. The request of the remote-request control never reaches a socket, because the driver refuses it first, so the check has nothing to count for it. The spike's Node code reaches its servers by address, not by the name localhost: the check ends a Node process that looks up that name.`
    : `This run does not count: ${outboundFaults.join('; ')}. The check's output is in .tmp/s4/sandbox.txt.`,
  '',
  '## Controls',
  '',
  '| control | what the run showed | failed as it must |',
  '|---|---|---|',
  ...controls.map((entry) => `| ${entry.label} | ${entry.showed} | ${entry.failed ? 'yes' : 'NO: the result does not count'} |`),
  '',
  '## How each value was measured, and what limits it',
  '',
  `- Actions. Each number is the length of the driver's action log for one run of the path. The one wrapper that performs a click, a typed field or a selection writes the entry (handle in spikes/s4/measure.mjs). ${dialogs === 0 ? 'Chromium reported no dialog in any run; one would have been logged as an action.' : `Chromium reported ${plural(dialogs, 'dialog')}, each logged as an action.`}`,
  '- Runs. One run of each path for each command. The counts are whole actions on fixed pages and carry no timing, so a second run writes the same file.',
  "- The page's own count. The stand-in page counts a field each time its value changes and the field is left, and a button each time it is clicked. It sends the count with the form, and the stand-in's server keeps it. The page does not tell a person's input from a script's, so the equality proves that the log and the page agree, not that a person could do the same. The page also sends the row of each input, and a run whose list differs from the log's does not count.",
  '- Reached. The state is what the stand-in backend holds when the run ends. The subscription becomes active when the payment page has sent a complete form with a valid test card. The cap is set when the one click in the app sends the default that the page shows. The backend, the app view, the sign-in page and the payment page were written for this spike. No Tauri app, no provider API and no signed webhook is in the path.',
  `- Payment page. It shows the rows of test-kit/checkout-fields.json. A US buyer fills ${plural(usCount.fields.total, 'field')} (${usCount.fields.docs} modelled on report rows, ${usCount.fields.unknown} unknown) and makes ${plural(usCount.clicks.total, 'click')} (${usCount.clicks.docs} from a report row, ${usCount.clicks.unknown} unknown: no row names the button that sends the form). A UK buyer fills ${gbCount.fields.total}. Nobody has seen these rows on the hosted page. What that page calls the name field and which countries get a postal code are open (researcher-5, Unknowns). Whether it fills in the country by itself is this spike's own open question.`,
  "- The app. The page shows the default cap above one Subscribe button, as the view of the plan's unit 8.1 does. The click sends that cap with the request that starts checkout, and the backend keeps it. The default, 20 USD a month, is a fixture in the page until R6 sets the product's number. The page of the first run had a second click, on a cap control that it showed after the payment.",
  `- Passkey first. ${plural(path.passkey?.passkeysInAuthenticator ?? 0, 'passkey')} was made in Playwright's virtual authenticator and Better Auth stored ${plural(authOf(path.passkey ?? {}).passkeys, 'passkey row')}. The virtual authenticator answers with no prompt, so the system prompt of a real authenticator is not in the count.`,
  '- Google-style sign-in. oauth2-mock-server answers the request at once and shows no page, so the provider\'s account choice or sign-in is not in the count. Better Auth passed the email from the mock to the payment page, which showed it locked, so this candidate types one field fewer there.',
  '- Sign-in page. The one click that each sign-in candidate logs there is on a button of the stand-in page. Better Auth and the mock ask for no click, and a real page could start the step by itself.',
  `- Better Auth rows. With IP tracking off and the user agent blanked by a hook, ${sessionRowsWithIdentity} of the ${plural(sessionRows, 'session row')} of the two sign-in candidates held an IP address or a user agent.`,
  '- Not counted. The path ends on the payment page. The driver then waits until the app page shows the state that the backend holds, which is no action. What the real app and browser show after the payment belongs to S5 and to phase 1. The plan also counts an answered OS dialog as a prompt (plan.md, done-when). No check on this Mac observes one, so none is in a count.',
  '',
  '## The payment page\'s rows by seller pair',
  '',
  ...pairCounts(fields, { country: 'US', emailLocked: false }).map(
    (pair) =>
      `- ${pair.label}: ${pair.unstated.length ? `at least ${plural(pair.fields, 'field')} for a US buyer; no number is stated for ${list(pair.unstated)}` : `${plural(pair.fields, 'field')} for a US buyer`}.`,
  ),
  ...fields.notMeasured.map((row) => `- ${row.label}: not measured (source ${sourceText(row.source)}).`),
  '',
  'Reproduce: `sh spikes/s4/run.sh` after `sh scripts/prerequisites.sh`. It runs the paths and the controls under the outbound sandbox, then rewrites this file above the next heading and gate/prompt-budget.json. A run with the same result on the same machine, with the same versions, leaves `git status` clean. Exit 0 for MET, 1 for MISSED, 2 when the run could not be made.',
];

await writeFile(RESULT, `${lines.join('\n')}\n${stated}`);
console.log(ruleLine);
console.log('s4: wrote spikes/s4/RESULT.md and gate/prompt-budget.json');
process.exit(met ? 0 : 1);
