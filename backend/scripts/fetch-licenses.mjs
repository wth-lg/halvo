import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { installedLicenses, LICENSE_LIST_READING, licenseOf, packagesOfEveryLockfileDocument } from "../gate/lib/lockfile.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const cacheFile = join(root, ".tools/licenses.json");
const packages = packagesOfEveryLockfileDocument(readFileSync(join(root, "pnpm-lock.yaml"), "utf8"));
const installed = installedLicenses(root);
const cached = existsSync(cacheFile) ? JSON.parse(readFileSync(cacheFile, "utf8")) : {};

const notInstalledOnThisPlatform = packages.filter((entry) => !installed.has(entry.id));
const notYetFetched = notInstalledOnThisPlatform.filter(
  (entry) => cached[entry.id]?.integrity !== entry.integrity || cached[entry.id]?.licenseList !== LICENSE_LIST_READING,
);

async function fetchLicense(entry) {
  const url = `https://registry.npmjs.org/${entry.name.replace("/", "%2F")}/${entry.version}`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url} answered ${response.status}`);
  const manifest = await response.json();
  if (manifest.dist?.integrity !== entry.integrity) {
    throw new Error(`${entry.id}: the registry's integrity differs from the lockfile's`);
  }
  return { license: licenseOf(manifest), integrity: entry.integrity, licenseList: LICENSE_LIST_READING };
}

const fetched = new Map();
const queue = [...notYetFetched];
await Promise.all(
  Array.from({ length: 12 }, async () => {
    for (let entry = queue.shift(); entry; entry = queue.shift()) {
      fetched.set(entry.id, await fetchLicense(entry));
    }
  }),
);

const next = {};
for (const entry of notInstalledOnThisPlatform.sort((a, b) => (a.id < b.id ? -1 : 1))) {
  next[entry.id] = fetched.get(entry.id) ?? cached[entry.id];
}
const text = `${JSON.stringify(next, null, 2)}\n`;
if (!existsSync(cacheFile) || readFileSync(cacheFile, "utf8") !== text) {
  mkdirSync(dirname(cacheFile), { recursive: true });
  writeFileSync(cacheFile, text);
}
console.log(
  `fetch-licenses: the lockfile lists ${packages.length} packages; ${installed.size} are installed, ` +
    `${notInstalledOnThisPlatform.length} are not; fetched the license of ${fetched.size}, already had ${notInstalledOnThisPlatform.length - fetched.size}`,
);
