import { readFileSync } from 'node:fs';

/** The category ids of contracts.md C0.5, in their order; `other` is last and is no seed category. */
export const CATEGORIES = [
  'utility_bill_payment',
  'tenant_rent',
  'lease_renewal_or_change',
  'tenant_application_review',
  'email',
  'spreadsheet_fill',
  'shop_order',
  'calendar_cleanup',
  'ride_order',
  'daily_chart_check',
  'other',
];

/** The test card number that the title of a card page holds (contracts.md C11.2). */
export const CARD_NUMBER = '4242 4242 4242 4242';

const SITE_KEYS = ['v', 'service', 'category', 'kind', 'host', 'app', 'parameters', 'values', 'pages', 'flows'];
const PAGE_KEYS = ['page', 'title', 'text', 'password_input', 'card_title'];
const FLOW_KEYS = ['flow', 'role', 'pages'];
const ROLES = ['task', 'near_miss', 'other'];
const CAPTURE_KEYS = ['category', 'site', 'trigger', 'recurrence'];
const SERVICE = /^[a-z0-9](?:[a-z0-9-]{0,30}[a-z0-9])?$/;
const ID = /^[a-z0-9-]{1,32}$/;
const NAME = /^[a-z][a-z0-9_]{0,31}$/;
const REFERENCE = /\{([a-z][a-z0-9_]{0,31})\}/g;
const UNRESERVED = /^[A-Za-z0-9\-._~]$/;
const TITLE_MAX = 1024;

/** Whether a value of --port is a port: decimal digits, 0 to 65535, where 0 asks the system for a free port. */
export function isPort(value) {
  return /^[0-9]{1,5}$/.test(value) && Number(value) <= 65535;
}

/**
 * Reads the arguments of a command that takes one optional flag with a value and one or more files; the flag may
 * stand before, between or after the files. Returns `{ value, files }`, with `value` null where the flag is absent,
 * or null for a usage error: an unknown flag, the flag twice or without a value, a value that `accepts` refuses,
 * or no file.
 */
export function readArgs(argv, flag, accepts) {
  let value = null;
  const files = [];
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === flag) {
      if (value !== null || i + 1 >= argv.length || !accepts(argv[i + 1])) return null;
      value = argv[i + 1];
      i += 1;
    } else if (argv[i].startsWith('--')) {
      return null;
    } else {
      files.push(argv[i]);
    }
  }
  return files.length === 0 ? null : { value, files };
}

/** The error of a file that is not of contracts.md C11.1; `rule` names the rule that failed. */
export class SiteDataError extends Error {
  constructor(rule, detail) {
    super(`${rule}: ${detail}`);
    this.name = 'SiteDataError';
    this.rule = rule;
  }
}

function fail(rule, detail) {
  throw new SiteDataError(rule, detail);
}

function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function keysAre(value, keys, what) {
  if (!isObject(value)) fail('type', `${what} is not an object`);
  const have = Object.keys(value);
  if (have.length !== keys.length || have.some((key, i) => key !== keys[i])) fail('D1', `${what} keys`);
}

function string(value, what) {
  if (typeof value !== 'string') fail('type', `${what} is not a string`);
  if (!value.isWellFormed()) fail('string', `${what} holds a lone surrogate`);
  return value;
}

function array(value, what) {
  if (!Array.isArray(value)) fail('type', `${what} is not an array`);
  return value;
}

function boolean(value, what) {
  if (typeof value !== 'boolean') fail('type', `${what} is not a boolean`);
}

function spacedWell(text) {
  return !text.startsWith(' ') && !text.endsWith(' ') && !text.includes('  ');
}

function appId(value) {
  if (typeof value !== 'string' || !value.isWellFormed()) return false;
  const size = Buffer.byteLength(value, 'utf8');
  if (size < 1 || size > 255 || value === '.' || value === '..') return false;
  return !/[/\\\u0000-\u001f\u007f-\u009f]/.test(value);
}

/** The names that a template refers to, in the order in which they stand. */
export function references(template) {
  return [...template.matchAll(REFERENCE)].map((match) => match[1]);
}

/** The template with each reference replaced by the value of its name, read once, left to right. */
export function fill(template, values) {
  return template.replace(REFERENCE, (_, name) => values.get(name));
}

/** A value percent-encoded byte by byte outside the unreserved set of RFC 3986 (contracts.md C10.9). */
export function encodeValue(value) {
  let out = '';
  for (const byte of Buffer.from(value, 'utf8')) {
    const char = String.fromCharCode(byte);
    out += UNRESERVED.test(char) ? char : `%${byte.toString(16).toUpperCase().padStart(2, '0')}`;
  }
  return out;
}

function compact(value) {
  return JSON.stringify(value);
}

function compactPage(page) {
  return `{"page":${compact(page.page)},"title":${compact(page.title)},"text":${compact(page.text)},"password_input":${page.password_input},"card_title":${page.card_title}}`;
}

function compactFlow(flow) {
  return `{"flow":${compact(flow.flow)},"role":${compact(flow.role)},"pages":[${flow.pages.map(compact).join(',')}]}`;
}

/** The pages and the flows of a site, written compact in the key order of C11.1: the body of the API face. */
export function compactPagesAndFlows(site) {
  return `{"pages":[${site.pages.map(compactPage).join(',')}],"flows":[${site.flows.map(compactFlow).join(',')}]}`;
}

function compactSite(site) {
  const values = site.parameters.map((name) => `${compact(name)}:[${site.values[name].map(compact).join(',')}]`).join(',');
  return `{"v":1,"service":${compact(site.service)},"category":${compact(site.category)},"kind":${compact(site.kind)},`
    + `"host":${compact(site.host)},"app":${compact(site.app)},"parameters":[${site.parameters.map(compact).join(',')}],`
    + `"values":{${values}},"pages":[${site.pages.map(compactPage).join(',')}],"flows":[${site.flows.map(compactFlow).join(',')}]}`;
}

function checkShape(site) {
  keysAre(site, SITE_KEYS, 'the object');
  if (site.v !== 1) fail('D1', 'v is not 1');
  string(site.service, 'service');
  if (!SERVICE.test(site.service)) fail('service', 'service name');
  string(site.category, 'category');
  if (!CATEGORIES.includes(site.category)) fail('category', 'category id');
  if (site.kind === 'web') {
    if (site.host !== `${site.service}.test` || site.app !== null) fail('kind', 'a web service has host <service>.test and app null');
  } else if (site.kind === 'native') {
    if (site.host !== null || site.app === null) fail('kind', 'a native service has host null and an app');
    if (!appId(site.app)) fail('app', 'app id');
  } else {
    fail('kind', 'kind is neither web nor native');
  }
  for (const name of array(site.parameters, 'parameters')) {
    if (!NAME.test(string(name, 'a parameter name'))) fail('parameter', 'parameter name');
  }
  if (!isObject(site.values)) fail('type', 'values is not an object');
  const valueKeys = Object.keys(site.values);
  if (valueKeys.length !== site.parameters.length || valueKeys.some((key, i) => key !== site.parameters[i])) {
    fail('D2', 'the keys of values are not the parameters in order');
  }
  for (const name of site.parameters) {
    const list = array(site.values[name], 'a value list');
    if (list.length === 0) fail('D2', 'an empty value list');
    for (const value of list) {
      if (typeof value !== 'string' || !value.isWellFormed()) fail('value', 'a value is not a string');
      const size = Buffer.byteLength(value, 'utf8');
      if (size < 1 || size > 64 || /[\u0000-\u001f\u007f]/.test(value)) fail('value', 'value form');
      if (!spacedWell(value)) fail('D10', 'a value has a space at an end or two in a row');
    }
    if (new Set(list).size !== list.length) fail('D2', 'a value stands twice');
  }
  for (const page of array(site.pages, 'pages')) {
    keysAre(page, PAGE_KEYS, 'a page');
    if (!ID.test(string(page.page, 'a page id')) || page.page === 'api') fail('id', 'page id');
    string(page.title, 'a title');
    string(page.text, 'a text');
    boolean(page.password_input, 'password_input');
    boolean(page.card_title, 'card_title');
  }
  for (const flow of array(site.flows, 'flows')) {
    keysAre(flow, FLOW_KEYS, 'a flow');
    if (!ID.test(string(flow.flow, 'a flow id'))) fail('id', 'flow id');
    string(flow.role, 'a role');
    if (!ROLES.includes(flow.role)) fail('role', 'role');
    for (const id of array(flow.pages, 'the pages of a flow')) string(id, 'a page of a flow');
  }
}

function filledTitleOk(site, title) {
  const plain = title.replace(REFERENCE, '');
  if (/[\u0000-\u001f\u007f]/.test(plain)) return false;
  let low = Buffer.byteLength(plain, 'utf8');
  let high = low;
  for (const name of references(title)) {
    const sizes = (site.values[name] ?? []).map((value) => Buffer.byteLength(value, 'utf8'));
    if (sizes.length > 0) {
      low += Math.min(...sizes);
      high += Math.max(...sizes);
    }
  }
  return low >= 1 && high <= TITLE_MAX;
}

function checkRules(site) {
  const seed = site.category !== 'other';
  if (site.pages.length === 0) fail('D3', 'no page');
  const pageIds = site.pages.map((page) => page.page);
  if (new Set(pageIds).size !== pageIds.length) fail('D3', 'a page id stands twice');
  if (new Set(site.flows.map((flow) => flow.flow)).size !== site.flows.length) fail('D3', 'a flow id stands twice');
  const used = new Set();
  for (const page of site.pages) {
    for (const name of [...references(page.title), ...references(page.text)]) {
      if (!site.parameters.includes(name)) fail('D4', 'a reference to a name that is not a parameter');
      used.add(name);
    }
  }
  for (const flow of site.flows) {
    if (flow.pages.length === 0 || flow.pages.some((id) => !pageIds.includes(id))) fail('D5', 'flow pages');
  }
  const roles = site.flows.map((flow) => flow.role);
  const count = (role) => roles.filter((r) => r === role).length;
  if (seed && (count('task') !== 1 || count('near_miss') !== 1 || count('other') !== 0)) fail('D6', 'seed flows');
  if (!seed && (roles.length === 0 || roles.some((role) => role !== 'other'))) fail('D6', 'other flows');
  const task = site.flows.find((flow) => flow.role === 'task');
  if (seed && site.kind === 'web') {
    const password = site.pages.filter((page) => page.password_input);
    if (password.length !== 1 || task.pages[0] !== password[0].page) fail('D7', 'the password page');
  }
  if (site.category === 'shop_order') {
    const card = site.pages.filter((page) => page.card_title);
    if (card.length !== 1 || !task.pages.includes(card[0].page)) fail('D7', 'the card page');
  }
  if (site.pages.some((page) => page.card_title && !page.title.includes(CARD_NUMBER))) fail('D7', 'a card title without the number');
  if (site.parameters.some((name) => CAPTURE_KEYS.includes(name))) fail('D9', 'a parameter named as a capture key');
  if (!seed && site.parameters.length !== 0) fail('D9', 'a service of the category other with parameters');
  if (site.pages.some((page) => !spacedWell(page.title))) fail('D10', 'a title has a space at an end or two in a row');
  if (site.parameters.some((name) => !used.has(name))) fail('unused', 'a parameter that no template uses');
  if (site.pages.some((page) => !filledTitleOk(site, page.title))) fail('D12', 'a filled title is empty, over 1,024 bytes or holds a control character');
}

/**
 * Reads the bytes of one site data file (contracts.md C11.1) and returns the parsed object, or throws a
 * SiteDataError. The file is taken only when it is byte-equal to its own compact rewriting in the listed key
 * order, followed by one line feed, and when the rules D1 to D7 and D9 to D13 hold. D8 is a rule over several
 * files and is checked by `loadSites`.
 */
export function parseSiteData(bytes) {
  let text;
  try {
    text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    fail('utf8', 'the bytes are not UTF-8');
  }
  let site;
  try {
    site = JSON.parse(text);
  } catch {
    fail('json', 'the text is not JSON');
  }
  checkShape(site);
  if (!Buffer.from(`${compactSite(site)}\n`, 'utf8').equals(bytes)) fail('form', 'not the compact rewriting');
  checkRules(site);
  return site;
}

/**
 * Reads the site data files at the given paths, in order. Returns `{ sites }` or `{ refused: path }` for the first
 * file that is missing, is not of C11.1, or names a service that an earlier file names (D8).
 */
export function loadSites(paths) {
  const sites = [];
  const services = new Set();
  for (const path of paths) {
    let site;
    try {
      site = parseSiteData(readFileSync(path));
    } catch (error) {
      if (error instanceof SiteDataError || error.code !== undefined) return { refused: path };
      throw error;
    }
    if (services.has(site.service)) return { refused: path };
    services.add(site.service);
    sites.push(site);
  }
  return { sites };
}

/** The names that a page uses, in the order of the service's `parameters`. */
export function namesOf(site, page) {
  const used = new Set([...references(page.title), ...references(page.text)]);
  return site.parameters.filter((name) => used.has(name));
}

/**
 * Every request that the page face answers for a service, in page order and, inside a page, in the order of the
 * value lists: `{ page, path, values, title, text }`, where `path` is the path and query of contracts.md C10.9.
 */
export function requestsOf(site) {
  const out = [];
  for (const page of site.pages) {
    const names = namesOf(site, page);
    let choices = [[]];
    for (const name of names) {
      choices = choices.flatMap((chosen) => site.values[name].map((value) => [...chosen, value]));
    }
    for (const chosen of choices) {
      const values = new Map(names.map((name, i) => [name, chosen[i]]));
      const query = names.map((name, i) => `${name}=${encodeValue(chosen[i])}`).join('&');
      out.push({
        page,
        path: names.length === 0 ? `/${page.page}` : `/${page.page}?${query}`,
        values,
        title: fill(page.title, values),
        text: fill(page.text, values),
      });
    }
  }
  return out;
}
