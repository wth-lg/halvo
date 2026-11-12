import { buyerCanActOn } from '/fields.mjs';

const base = location.pathname.replace(/\/$/, '');
const [fields, session] = await Promise.all([
  fetch('/fields.json').then((response) => response.json()),
  fetch(`${base}/session.json`).then((response) => response.json()),
]);
const form = document.getElementById('checkout');
const status = document.getElementById('status');
const emailLocked = session.customerEmail !== null;
const changesAndClicks = [];

function isLockedEmail(row) {
  return row.lockedBy && emailLocked;
}

function control(row) {
  if (row.kind === 'click') {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = row.label;
    return button;
  }
  if (row.options) {
    const select = document.createElement('select');
    select.append(new Option('', ''));
    for (const option of row.options) select.append(new Option(option.label, option.value));
    return select;
  }
  const input = document.createElement('input');
  input.type = row.input ?? 'text';
  if (isLockedEmail(row)) {
    input.value = session.customerEmail;
    input.readOnly = true;
  }
  return input;
}

for (const row of fields.rows) {
  if (row.render === false) continue;
  const element = control(row);
  element.name = row.id;
  element.dataset.row = row.id;
  const line = document.createElement('p');
  line.dataset.line = row.id;
  if (row.kind === 'click') {
    line.append(element);
  } else {
    const label = document.createElement('label');
    label.append(`${row.label} `, element);
    line.append(label);
  }
  form.append(line);
}

const current = () => ({ country: form.elements.country.value, emailLocked });

function refresh() {
  for (const row of fields.rows) {
    const line = form.querySelector(`[data-line="${row.id}"]`);
    if (line) line.hidden = !(buyerCanActOn(row, current()) || isLockedEmail(row));
  }
}

async function pay() {
  const values = {};
  for (const row of fields.rows) {
    const element = form.elements[row.id];
    if (!element || row.kind === 'click' || !buyerCanActOn(row, current())) continue;
    values[row.id] = element.type === 'checkbox' ? element.checked : element.value;
  }
  const response = await fetch(`${base}/pay`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ values, inputs: { count: changesAndClicks.length, ids: changesAndClicks } }),
  });
  const result = await response.json();
  status.textContent = result.ok
    ? 'Subscription started. You can go back to the app.'
    : `Not sent: ${result.error}.`;
  status.dataset.state = result.ok ? 'paid' : 'refused';
  form.hidden = result.ok;
}

form.addEventListener('change', (event) => {
  if (event.target.dataset.row && !event.target.readOnly) changesAndClicks.push(event.target.dataset.row);
  refresh();
});
form.addEventListener('click', (event) => {
  const button = event.target.closest('button[data-row]');
  if (!button) return;
  changesAndClicks.push(button.dataset.row);
  if (fields.rows.find((row) => row.id === button.dataset.row)?.submits) pay();
});

refresh();
form.dataset.ready = 'yes';
