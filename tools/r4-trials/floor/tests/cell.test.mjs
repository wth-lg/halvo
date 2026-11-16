import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import { cellLine, machineOf, missLine, parseProbe, systemOf } from '../cell.mjs';
import { POLICY } from '../headless.mjs';
import { LIST, parseList, probeSource, ROOT } from '../list.mjs';
import { parseCell } from '../result.mjs';
import { hostedEnv, standInPath, withPath } from './stand-in-cargo.mjs';

const HERE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const MAC_AGENT = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)';
const WIN_AGENT = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.3595.94';

function probeLine(features, agent, aria) {
  const lines = [];
  const context = {
    navigator: { userAgent: agent },
    document: aria ? { ariaNotify() {} } : {},
    window: { r4app: { report: (line) => lines.push(line) } },
  };
  vm.runInNewContext(probeSource(features), context);
  assert.equal(lines.length, 1);
  return lines[0];
}

function env(extra) {
  const copy = withPath(standInPath(), extra);
  for (const key of ['CI', 'R4_GUEST']) if (!(key in extra)) delete copy[key];
  return copy;
}

const allow = (name, cell) => ({ kind: 'allow', name, version: '1', cell, line: 0 });

/** The cases of the probe, the cell line and the refusals of run.sh and cell.mjs. */
export const cases = [
  ['probe_counts_a_feature_missing_when_its_expression_is_not_true', () => {
    const features = [allow('yes', 'true'), allow('no', 'false'), allow('throws', 'undefinedName.x'), allow('truthy', '1'), allow('dep', "typeof Intl === 'object'")];
    assert.equal(probeLine(features, MAC_AGENT, false),
      'probe features=5 missing=3 missing_names=no,throws,truthy webkit=605.1.15 edg=- aria_notify=undefined');
  }],
  ['probe_reads_the_edg_version_and_aria_notify', () => {
    assert.equal(probeLine([allow('yes', 'true')], WIN_AGENT, true),
      'probe features=1 missing=0 missing_names=- webkit=537.36 edg=142.0.3595.94 aria_notify=function');
  }],
  ['probe_of_the_list_evaluates_every_allow_line', () => {
    const features = parseList(fs.readFileSync(LIST, 'utf8')).features;
    const source = probeSource(features);
    const n = features.filter((f) => f.kind === 'allow').length;
    assert.equal((source.match(/^ {2}\["[a-z0-9_]+", function \(\) \{ return \(/gm) ?? []).length, n);
    for (const f of features.filter((x) => x.kind === 'allow')) assert.ok(source.includes(`["${f.name}", function () { return (${f.cell}); }]`), f.name);
  }],
  ['the_cell_line_has_the_form_of_i11_4', () => {
    const probe = parseProbe('page probe features=74 missing=0 missing_names=- webkit=605.1.15 edg=- aria_notify=undefined');
    const mac = cellLine('hosted-mac', 'darwin', '15.7.1', probe);
    assert.equal(mac, 'cell trial=floor machine=hosted-mac system=15.7.1 webkit=605.1.15 features=74 missing=0 missing_names=- aria_notify=undefined');
    assert.deepEqual(parseCell(mac), { machine: 'hosted-mac', system: '15.7.1', webkit: '605.1.15', features: 74, missing: 0, names: '-', aria: 'undefined' });
    const win = cellLine('hosted-win', 'win32', '10.0.26100.4652', parseProbe('page probe features=74 missing=1 missing_names=a_b webkit=537.36 edg=142.0.3595.94 aria_notify=function'));
    assert.equal(win, 'cell trial=floor machine=hosted-win system=10.0.26100.4652 webkit=142.0.3595.94 features=74 missing=1 missing_names=a_b aria_notify=function');
    assert.equal(missLine('hosted-win', 'quit'), 'miss trial=floor machine=hosted-win arm=main run=1 step=quit');
  }],
  ['the_probe_line_is_refused_out_of_form', () => {
    for (const line of [null, '', 'probe features=1 missing=0 missing_names=- webkit=1 edg=- aria_notify=undefined',
      'page probe features=1 missing=0 missing_names=A webkit=1 edg=- aria_notify=undefined',
      'page probe features=1 missing=0 missing_names=- webkit=1 edg=- aria_notify=object']) assert.equal(parseProbe(line), null);
  }],
  ['the_system_version_is_read_from_sw_vers_and_ver', () => {
    assert.equal(systemOf('darwin', '15.7.1\n'), '15.7.1');
    assert.equal(systemOf('darwin', '14.0\n'), '14.0');
    assert.equal(systemOf('win32', '\r\nMicrosoft Windows [Version 10.0.26100.4652]\r\n'), '10.0.26100.4652');
    assert.equal(systemOf('darwin', 'ProductVersion: 15.7.1\n'), null);
    assert.equal(systemOf('win32', 'Microsoft Windows\r\n'), null);
    assert.equal(systemOf('win32', 'Node 26.0.0\r\n'), null);
  }],
  ['the_machine_follows_r4_guest_and_the_system', () => {
    assert.equal(machineOf({ R4_GUEST: '1' }, 'darwin'), 'guest-mac14');
    assert.equal(machineOf({ CI: 'true' }, 'darwin'), 'hosted-mac');
    assert.equal(machineOf({ CI: 'true' }, 'win32'), 'hosted-win');
    assert.equal(machineOf({ CI: 'true', R4_GUEST: '' }, 'linux'), null);
  }],
  ['run_sh_refuses_where_neither_ci_nor_r4_guest_is_set', () => {
    for (const extra of [{}, { CI: '' }, { R4_GUEST: '' }]) {
      const r = spawnSync('sh', [path.join(HERE, 'run.sh')], { encoding: 'utf8', env: env(extra), cwd: ROOT });
      assert.equal(r.status, 2);
      assert.equal(r.stdout, '');
      assert.match(r.stderr, /^REFUSED not_hosted: [^\n]*\n$/);
    }
  }],
  ['run_sh_refuses_an_argument_before_it_builds', () => {
    const r = spawnSync('sh', [path.join(HERE, 'run.sh'), '--arm'], { encoding: 'utf8', env: hostedEnv('true', standInPath()), cwd: ROOT });
    assert.equal(r.status, 2);
    assert.equal(r.stdout, '');
    assert.match(r.stderr, /^REFUSED usage: [^\n]*\n$/);
  }],
  ['cell_mjs_refuses_usage_and_not_hosted', () => {
    const cellPath = path.join(HERE, 'cell.mjs');
    const usage = spawnSync(process.execPath, [cellPath], { encoding: 'utf8', env: hostedEnv('true', standInPath()) });
    assert.equal(usage.status, 2);
    assert.equal(usage.stdout, '');
    assert.match(usage.stderr, /^REFUSED usage: [^\n]*\n$/);
    const hosted = spawnSync(process.execPath, [cellPath, '--bin', 'x'], { encoding: 'utf8', env: env({}) });
    assert.equal(hosted.status, 2);
    assert.equal(hosted.stdout, '');
    assert.match(hosted.stderr, /^REFUSED not_hosted: [^\n]*\n$/);
  }],
  ['the_stand_in_policy_is_the_policy_of_the_app', () => {
    const rust = fs.readFileSync(path.join(ROOT, 'tools', 'r4-trials', 'app', 'src', 'pages.rs'), 'utf8');
    const m = /"(default-src 'none'; script-src 'self' \{\}; connect-src \{ipc\}; base-uri 'none'; )\\\s*\n\s*(form-action 'none'; frame-ancestors 'none')"/.exec(rust);
    assert.ok(m, 'the policy text of pages.rs');
    assert.equal(POLICY, (m[1] + m[2]).replace('{}', 'http://floor.localhost').replace('{ipc}', 'ipc://localhost'));
    assert.ok(!/unsafe-eval/.test(rust));
  }],
];
