import { existsSync, lstatSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

export function packagesOfEveryLockfileDocument(text) {
  const packages = new Map();
  let inPackages = false;
  let current = null;
  for (const line of text.split("\n")) {
    if (/^\S/.test(line)) {
      inPackages = line === "packages:";
      current = null;
      continue;
    }
    if (!inPackages) continue;
    const key = /^ {2}(\S.*):$/.exec(line);
    if (key) {
      const id = key[1].replace(/^'(.*)'$/, "$1");
      const at = id.lastIndexOf("@");
      current = { id, name: id.slice(0, at), version: id.slice(at + 1), integrity: null };
      packages.set(id, current);
      continue;
    }
    const integrity = /^ {4}resolution: \{integrity: ([^,}]+)/.exec(line);
    if (current && integrity) current.integrity = integrity[1];
  }
  return [...packages.values()];
}

export function licenseOf(manifest) {
  const value = manifest.license ?? manifest.licenses;
  if (typeof value === "string") return value;
  if (Array.isArray(value)) {
    const names = value.map((entry) => (typeof entry === "string" ? entry : entry?.type)).filter(Boolean);
    return names.length > 0 ? names.join(" OR ") : null;
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
    if (integrity && entry?.integrity === integrity) licenses.set(id, entry.license);
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
