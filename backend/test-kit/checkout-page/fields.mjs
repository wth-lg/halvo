export const KINDS = ['typed', 'selection', 'click'];
export const CLASSES = ['start', 'card', 'cap', 'outside'];
export const WHERES = ['app', 'checkout page', 'browser', 'operating system'];
export const NOT_MEASURED_IDS = ['three-d-secure', 'wallet-prompt'];

function fail(message) {
  throw new Error(`checkout-fields.json: ${message}`);
}

function checkSource(owner, source) {
  if (source?.type === 'unknown') return;
  const rows = source?.type === 'docs' && Array.isArray(source.rows) ? source.rows : [];
  if (rows.length > 0 && rows.every((row) => /^r[1-5]#\d+$/.test(row))) return;
  fail(`${owner}: the source must be {"type":"unknown"} or {"type":"docs","rows":["r1#93"]}`);
}

export function validateFields(fields) {
  const ids = new Set();
  for (const row of fields.rows ?? fail('rows is missing')) {
    if (!row.id || ids.has(row.id)) fail(`row ${row.id}: the id is missing or repeated`);
    ids.add(row.id);
    if (!KINDS.includes(row.kind)) fail(`row ${row.id}: kind ${row.kind}`);
    if (!CLASSES.includes(row.class)) fail(`row ${row.id}: class ${row.class}`);
    if (typeof row.required !== 'boolean') fail(`row ${row.id}: required must be true or false`);
    checkSource(`row ${row.id}`, row.source);
  }
  for (const pair of fields.sellerPairs ?? fail('sellerPairs is missing')) {
    for (const added of pair.adds) {
      checkSource(`seller pair ${pair.id}, row ${added.id}`, added.source);
      if (added.inputs !== null && !Number.isInteger(added.inputs)) {
        fail(`seller pair ${pair.id}, row ${added.id}: inputs must be a whole number, or null when the report row states none`);
      }
    }
  }
  const notMeasured = fields.notMeasured ?? fail('notMeasured is missing');
  for (const row of notMeasured) {
    if (row.status !== 'not measured') fail(`row ${row.id}: the status must be "not measured"`);
    if ('count' in row || 'inputs' in row) fail(`row ${row.id}: a row that is not measured carries no count`);
    if (!WHERES.includes(row.where)) fail(`row ${row.id}: where ${row.where}`);
    if (!CLASSES.includes(row.class)) fail(`row ${row.id}: class ${row.class}`);
    checkSource(`row ${row.id}`, row.source);
  }
  for (const id of NOT_MEASURED_IDS) {
    if (!notMeasured.some((row) => row.id === id)) fail(`the not-measured row ${id} is missing`);
  }
  return fields;
}

export function buyerCanActOn(row, { country, emailLocked }) {
  if (row.render === false) return false;
  if (row.lockedBy && emailLocked) return false;
  if (row.when?.country && !row.when.country.includes(country)) return false;
  return true;
}

export function requiredRows(fields, session) {
  return fields.rows.filter((row) => row.required && buyerCanActOn(row, session));
}

export function countFor(fields, session) {
  const rows = requiredRows(fields, session);
  const split = (list) => {
    const docs = list.filter((row) => row.source.type === 'docs').length;
    return { total: list.length, docs, unknown: list.length - docs };
  };
  return {
    fields: split(rows.filter((row) => row.kind !== 'click')),
    clicks: split(rows.filter((row) => row.kind === 'click')),
    typed: rows.filter((row) => row.kind === 'typed').length,
  };
}

export function pairCounts(fields, session) {
  const base = countFor(fields, session).fields.total;
  return fields.sellerPairs.map((pair) => ({
    id: pair.id,
    label: pair.label,
    fields: base + pair.adds.reduce((sum, row) => sum + (row.inputs ?? 0), 0),
    unstated: pair.adds
      .filter((row) => row.inputs === null)
      .map((row) => `${row.label} (${row.source.type === 'docs' ? row.source.rows.join(', ') : 'source unknown'})`),
  }));
}
