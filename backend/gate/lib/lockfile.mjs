import { existsSync, lstatSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

export function packagesOfEveryLockfileDocument(text) {
  const packages = new Map();
  let inPackages = false;
  let current = null;
  for (const [index, line] of text.split("\n").entries()) {
    if (/^\S/.test(line)) {
      if (line !== "packages:" && /^['"]?packages['"]?\s*:/.test(line)) {
        throw new Error(`pnpm-lock.yaml line ${index + 1} ${JSON.stringify(line)} holds the key packages in another form than the line "packages:"`);
      }
      inPackages = line === "packages:";
      current = null;
      continue;
    }
    if (!inPackages || line.trim() === "") continue;
    const key = /^ {2}(\S.*):$/.exec(line);
    if (key) {
      const id = key[1].replace(/^'(.*)'$/, "$1");
      const at = id.lastIndexOf("@");
      current = { id, name: id.slice(0, at), version: id.slice(at + 1), integrity: null };
      packages.set(id, current);
      continue;
    }
    if (!current || !/^ {4}/.test(line)) {
      throw new Error(
        `pnpm-lock.yaml line ${index + 1} ${JSON.stringify(line)} is in a packages section and is no blank line, no key line "  <name>@<version>:" and no deeper line of an entry`,
      );
    }
    const integrity = /^ {4}resolution: \{integrity: ([^,}]+)/.exec(line);
    if (integrity) current.integrity = integrity[1];
  }
  return [...packages.values()];
}

export const LICENSE_LIST_READING = "AND";

export function licenseOf(manifest) {
  const value = manifest.license ?? manifest.licenses;
  if (typeof value === "string") return value;
  if (Array.isArray(value)) {
    const names = value.map((entry) => (typeof entry === "string" ? entry : entry?.type));
    if (names.length === 0 || names.some((name) => typeof name !== "string" || name.trim() === "")) return null;
    return names.map((name) => `(${name})`).join(` ${LICENSE_LIST_READING} `);
  }
  if (value && typeof value.type === "string") return value.type;
  return null;
}

export function cachedLicenses(root, packages) {
  const path = join(root, ".tools/licenses.json");
  const entries = existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : {};
  const integrityOf = new Map(packages.map((entry) => [entry.id, entry.integrity]));
  const licenses = new Map();
  const untied = [];
  for (const [id, entry] of Object.entries(entries)) {
    const integrity = integrityOf.get(id);
    if (integrity && entry?.integrity === integrity && entry.licenseList === LICENSE_LIST_READING) licenses.set(id, entry.license);
    else untied.push(id);
  }
  return { licenses, untied };
}

const isDependencyLink = (path) => lstatSync(path).isSymbolicLink();

export function installedLicenses(root) {
  const licenses = new Map();
  const store = join(root, "node_modules/.pnpm");
  if (!existsSync(store)) return licenses;
  for (const entry of readdirSync(store)) {
    const modules = join(store, entry, "node_modules");
    if (!existsSync(modules)) continue;
    for (const name of readdirSync(modules)) {
      const path = join(modules, name);
      if (isDependencyLink(path)) continue;
      const directories = name.startsWith("@")
        ? readdirSync(path)
            .map((child) => join(path, child))
            .filter((child) => !isDependencyLink(child))
        : [path];
      for (const directory of directories) {
        const manifestPath = join(directory, "package.json");
        if (!existsSync(manifestPath)) continue;
        const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
        if (manifest.name && manifest.version) {
          licenses.set(`${manifest.name}@${manifest.version}`, licenseOf(manifest));
        }
      }
    }
  }
  return licenses;
}
