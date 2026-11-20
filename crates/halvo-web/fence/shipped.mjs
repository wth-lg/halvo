import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';

function usage() {
  process.stdout.write(`Usage: node crates/halvo-web/fence/shipped.mjs [--control | --self-test]

The check of the fence of halvo-web (CP-186 N8, in the form of the capture run's W3-8): builds
what ships of the crate by one cargo call that makes no test target,
"cargo build --locked --release -p halvo-web", from the root of the checkout, and reads every
file that the call reports as an artifact of crates/halvo-web for the fence's marker, the byte
string that stands in a build with the feature test-fence and in no build without it. It prints
one line per file, "shipped file=<name> bytes=<n> program=<yes|no> marker=<yes|no>" (a program
is a file that the call reports as an executable), and the verdict,
"shipped files=<n> with_marker=<n> programs_without_marker=<n> result=<PASS|FAIL>": PASS when it
read at least one file and none holds the marker. With --control the call names the feature as
well ("--features test-fence"), and the verdict is PASS when the call reported at least one
program and every program holds the marker: a call that reported no program, and a program that
is built only with the feature (web-probe, web-rows, web-standin) and lacks the marker, fail the
control. --self-test reads files that it writes itself, rows of the verdict and lines in the
form of cargo's messages, from which it wants the files and the programs named above, and runs
no cargo. Exit 0 for PASS, 1 for FAIL, 2 for a refusal: one line
REFUSED <usage|cargo>: <detail> on standard error. It prints file names, sizes and words alone.
`);
}

function refuse(cause, detail) {
  process.stderr.write(`REFUSED ${cause}: ${detail}\n`);
  process.exit(2);
}

const MARKER = Buffer.from(['halvo-web', 'test-fence', 'this-build-has-it!'].join(':'));

function holds(bytes) {
  return bytes.indexOf(MARKER) >= 0;
}

function verdict(read, control) {
  const withMarker = read.filter((r) => r.marker).length;
  const programs = read.filter((r) => r.program).length;
  const programsWithout = read.filter((r) => r.program && !r.marker).length;
  const pass = read.length > 0 && (control ? programs > 0 && programsWithout === 0 : withMarker === 0);
  return { withMarker, programsWithout, pass };
}

function report(read, control) {
  for (const r of read) {
    process.stdout.write(
      `shipped file=${r.name} bytes=${r.bytes} program=${r.program ? 'yes' : 'no'} marker=${r.marker ? 'yes' : 'no'}\n`,
    );
  }
  const { withMarker, programsWithout, pass } = verdict(read, control);
  const kind = control ? 'shipped control' : 'shipped';
  process.stdout.write(
    `${kind} files=${read.length} with_marker=${withMarker} programs_without_marker=${programsWithout} result=${pass ? 'PASS' : 'FAIL'}\n`,
  );
  return pass;
}

function readFile(path, program = false) {
  const bytes = readFileSync(path);
  return { name: basename(path), bytes: bytes.length, program, marker: holds(bytes) };
}

function artifacts(stdout) {
  const files = new Map();
  for (const line of stdout.split('\n')) {
    if (!line.startsWith('{')) continue;
    let m;
    try {
      m = JSON.parse(line);
    } catch {
      continue;
    }
    if (m.reason !== 'compiler-artifact') continue;
    const manifest = String(m.manifest_path || '').replaceAll('\\', '/');
    if (!manifest.endsWith('/crates/halvo-web/Cargo.toml')) continue;
    for (const f of m.filenames || []) if (!files.has(f)) files.set(f, false);
    if (m.executable) files.set(m.executable, true);
  }
  return [...files.entries()].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
}

function selfTest() {
  const dir = mkdtempSync(join(tmpdir(), 'halvo-web-shipped-'));
  const cases = [
    ['marker', Buffer.concat([Buffer.from([0, 255, 7]), MARKER, Buffer.from([0])]), true],
    ['clean', Buffer.from([0, 1, 2, 3, 255]), false],
    ['part', MARKER.subarray(0, MARKER.length - 1), false],
    ['empty', Buffer.alloc(0), false],
  ];
  let bad = 0;
  for (const [name, bytes, want] of cases) {
    const path = join(dir, name);
    writeFileSync(path, bytes);
    const got = readFile(path).marker;
    if (got !== want) {
      bad += 1;
      process.stdout.write(`self-test case=${name} marker=${got ? 'yes' : 'no'} want=${want ? 'yes' : 'no'}\n`);
    }
  }
  const row = (marker) => ({ name: 'f', bytes: 1, marker });
  const prog = (marker) => ({ name: 'p', bytes: 1, marker, program: true });
  const verdicts = [
    [[prog(true), prog(false)], true, false],
    [[row(true), prog(true), prog(false)], true, false],
    [[row(true), prog(true), row(false)], true, true],
    [[row(false), prog(true)], true, true],
    [[row(false)], false, true],
    [[row(false), row(true)], false, false],
    [[], false, false],
    [[row(true)], true, false],
    [[row(true), row(true)], true, false],
    [[prog(true)], true, true],
    [[row(false)], true, false],
    [[], true, false],
  ];
  for (const [read, control, want] of verdicts) {
    if (verdict(read, control).pass !== want) {
      bad += 1;
      process.stdout.write(`self-test verdict files=${read.length} control=${control} want=${want}\n`);
    }
  }
  const message = (reason, manifest, filenames, executable) =>
    JSON.stringify({ reason, manifest_path: manifest, filenames, executable });
  const web = '/w/crates/halvo-web/Cargo.toml';
  const rlib = message('compiler-artifact', web, ['/w/r/libhalvo_web.rlib', '/w/r/deps/libhalvo_web-1.rmeta'], null);
  const bin = message('compiler-artifact', web, ['/w/r/web-probe'], '/w/r/web-probe');
  const winBin = message(
    'compiler-artifact',
    'C:\\w\\crates\\halvo-web\\Cargo.toml',
    ['C:\\w\\r\\web-rows.exe', 'C:\\w\\r\\web_rows.pdb'],
    'C:\\w\\r\\web-rows.exe',
  );
  const other = message('compiler-artifact', '/w/crates/halvo-test-dir/Cargo.toml', ['/w/r/libhalvo_test_dir.rlib'], null);
  const notArtifact = message('compiler-message', web, ['/w/r/note'], '/w/r/note');
  const libFiles = [
    ['/w/r/deps/libhalvo_web-1.rmeta', false],
    ['/w/r/libhalvo_web.rlib', false],
  ];
  const winFiles = [
    ['C:\\w\\r\\web-rows.exe', true],
    ['C:\\w\\r\\web_rows.pdb', false],
  ];
  const lines = [
    ['rlib', [rlib], libFiles],
    ['bin', [bin], [['/w/r/web-probe', true]]],
    ['windows_bin', [winBin], winFiles],
    ['other_crate', [other], []],
    ['not_json', ['{"reason":"compiler-artifact",', 'Compiling halvo-web v0.0.0'], []],
    ['not_artifact', [notArtifact, JSON.stringify({ reason: 'build-finished', success: true })], []],
    [
      'all',
      ['Compiling halvo-web v0.0.0', other, winBin, '{not json', bin, notArtifact, rlib],
      [...libFiles, ['/w/r/web-probe', true], ...winFiles],
    ],
  ];
  for (const [name, stdout, want] of lines) {
    const got = artifacts(stdout.join('\n'));
    if (JSON.stringify(got) !== JSON.stringify(want)) {
      bad += 1;
      const programs = (list) => list.filter(([, program]) => program).length;
      process.stdout.write(
        `self-test artifacts case=${name} files=${got.length} programs=${programs(got)} want_files=${want.length} want_programs=${programs(want)}\n`,
      );
    }
  }
  rmSync(dir, { recursive: true, force: true });
  const count = cases.length + verdicts.length + lines.length;
  process.stdout.write(`self-test cases=${count} bad=${bad} result=${bad === 0 ? 'PASS' : 'FAIL'}\n`);
  return bad === 0;
}

const argv = process.argv.slice(2);
if (argv.includes('--help')) {
  usage();
  process.exit(0);
}
if (argv.length > 1 || (argv.length === 1 && !['--control', '--self-test'].includes(argv[0]))) {
  refuse('usage', 'see --help');
}
if (argv[0] === '--self-test') process.exit(selfTest() ? 0 : 1);
const control = argv[0] === '--control';
const args = ['build', '--locked', '--release', '-p', 'halvo-web', '--message-format=json-render-diagnostics'];
if (control) args.push('--features', 'test-fence');
const built = spawnSync('cargo', args, { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] });
if (built.error) refuse('cargo', `the call did not run (${built.error.code || built.error.name})`);
if (built.status !== 0) refuse('cargo', `the build exited ${built.status}`);
const read = artifacts(built.stdout).map(([path, program]) => readFile(path, program));
process.exit(report(read, control) ? 0 : 1);
