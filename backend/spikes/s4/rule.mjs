import { NOT_MEASURED_IDS } from '../../test-kit/checkout-page/fields.mjs';

const APP_CLICKS = 1;
const PAY_CLICKS = 1;
const DOCUMENTED_INPUTS = { us: 7, gb: 6 };
export const EXPECTED = {
  'device-us': APP_CLICKS + DOCUMENTED_INPUTS.us + PAY_CLICKS,
  'device-gb': APP_CLICKS + DOCUMENTED_INPUTS.gb + PAY_CLICKS,
};
export const BUDGET_PATH_ID = 'device-us';

const APP_SOURCES = {
  subscribe: {
    type: 'plan',
    ref: 'plan.md, units S4 and 8.1: the one click, which sets the default cap and starts checkout',
  },
};

export const reachedActiveWithCap = (record) =>
  record.error === null && record.reached?.subscription === 'active' && Boolean(record.reached.cap);
export const checkoutActions = (record) => record.log.filter((action) => action.where === 'checkout page').length;
export const logEqualsPage = (record) => record.page !== null && checkoutActions(record) === record.page.count;
export const classCount = (record, name) => record.log.filter((action) => action.class === name).length;
export const typedCount = (record) => record.log.filter((action) => action.kind === 'typed').length;

export function budgetRows(record, fields) {
  const sources = new Map(fields.rows.map((row) => [row.id, row.source]));
  const measured = record.log.map((action) => ({
    n: action.n,
    action: action.kind,
    target: action.target,
    where: action.where,
    class: action.class,
    source:
      action.where === 'checkout page'
        ? (sources.get(action.target) ?? { type: 'unknown' })
        : (APP_SOURCES[action.target] ?? { type: 'unknown' }),
    status: 'measured',
  }));
  const notMeasured = fields.notMeasured.map(({ id, label, where, class: rowClass, source, status }) => ({
    id,
    label,
    where,
    class: rowClass,
    source,
    status,
  }));
  return [...measured, ...notMeasured];
}

export function budgetRowFaults(rows, record) {
  const faults = [];
  const measured = rows.filter((row) => row.status === 'measured');
  const notMeasured = rows.filter((row) => row.status === 'not measured');
  if (measured.length + notMeasured.length !== rows.length) {
    faults.push('a row has a status other than measured or not measured');
  }
  if (measured.length !== record.log.length) {
    faults.push(`${measured.length} measured rows for ${record.log.length} logged actions`);
  }
  for (const [index, action] of record.log.entries()) {
    const row = measured[index];
    const same =
      row &&
      row.n === action.n &&
      row.action === action.kind &&
      row.target === action.target &&
      row.where === action.where &&
      row.class === action.class;
    if (row && !same) faults.push(`measured row ${index + 1} is not logged action ${action.n}`);
  }
  const ids = notMeasured.map((row) => row.id).sort();
  if (ids.join(',') !== [...NOT_MEASURED_IDS].sort().join(',')) {
    faults.push(`the rows that are not measured are [${ids.join(', ')}]`);
  }
  for (const row of notMeasured) {
    if ('count' in row || 'inputs' in row || 'n' in row) {
      faults.push(`the row ${row.id} is not measured and carries a count`);
    }
  }
  return faults;
}

export function alteredBudgets(rows) {
  const firstAction = rows.findIndex((row) => row.status === 'measured');
  const extra = {
    n: rows.length,
    action: 'click',
    target: 'extra',
    where: 'app',
    class: 'outside',
    source: { type: 'unknown' },
    status: 'measured',
  };
  return [
    { id: 'one row added', rows: [...rows, extra] },
    { id: 'one action row removed', rows: rows.filter((_, index) => index !== firstAction) },
    {
      id: 'the wallet prompt given a count',
      rows: rows.map((row) => (row.id === 'wallet-prompt' ? { ...row, count: 1 } : row)),
    },
  ];
}

export function rank(records, actionsForUnmeasuredRows = () => 0) {
  return [...records].sort(
    (a, b) =>
      classCount(a, 'outside') + actionsForUnmeasuredRows(a) - (classCount(b, 'outside') + actionsForUnmeasuredRows(b)) || typedCount(a) - typedCount(b),
  );
}
