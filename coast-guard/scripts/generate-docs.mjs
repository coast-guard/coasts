#!/usr/bin/env node
/**
 * Walks ../docs/ and builds a JSON manifest with tree structure + markdown
 * content for every locale.  Output: src/generated/docs-manifest.json
 */

import { readdir, readFile, stat, mkdir, writeFile, copyFile, rm } from "node:fs/promises";
import { join, relative, extname, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const DOCS_ROOT = join(__dirname, "..", "..", "docs");
const ROOT_ASSETS_DIR = join(__dirname, "..", "..", "assets");
const OUT_DIR = join(__dirname, "..", "src", "generated");
const OUT_FILE = join(OUT_DIR, "docs-manifest.json");
const DOCS_ASSETS_OUT_DIR = join(__dirname, "..", "public", "docs-assets");

const LOCALES = ["en", "zh", "ja", "ko", "ru", "pt", "es"];
const LOCALE_DIRS = new Set(LOCALES.filter((l) => l !== "en"));
const IGNORED = new Set([".DS_Store"]);

async function exists(p) {
  try {
    await stat(p);
    return true;
  } catch {
    return false;
  }
}

/**
 * Recursively walk a directory, returning a sorted tree of nodes and a flat
 * map of relativePath -> file contents (markdown only).
 */
async function walkDir(root, base = "") {
  const entries = (await readdir(root, { withFileTypes: true })).filter(
    (e) => !IGNORED.has(e.name)
  );

  const dirs = entries.filter((e) => e.isDirectory()).sort((a, b) => a.name.localeCompare(b.name));
  const files = entries
    .filter((e) => e.isFile() && extname(e.name) === ".md")
    .sort((a, b) => {
      if (a.name === "README.md") return -1;
      if (b.name === "README.md") return 1;
      return a.name.localeCompare(b.name);
    });

  const tree = [];
  const fileMap = {};

  for (const f of files) {
    const relPath = base ? `${base}/${f.name}` : f.name;
    const content = await readFile(join(root, f.name), "utf-8");
    fileMap[relPath] = content;
    tree.push({ name: f.name, path: relPath, type: "file" });
  }

  for (const d of dirs) {
    const relPath = base ? `${base}/${d.name}` : d.name;
    const sub = await walkDir(join(root, d.name), relPath);
    Object.assign(fileMap, sub.fileMap);
    tree.push({ name: d.name, path: relPath, type: "dir", children: sub.tree });
  }

  return { tree, fileMap };
}

async function buildLocale(locale) {
  const root = locale === "en" ? DOCS_ROOT : join(DOCS_ROOT, locale);
  if (!(await exists(root))) {
    return { tree: [], files: {} };
  }

  const skipDirs = locale === "en" ? LOCALE_DIRS : null;
  return walkLocaleDir(root, "", skipDirs);
}

/**
 * Like walkDir but optionally skips certain top-level directories (used for
 * the English root so we don't recurse into locale subdirs).
 */
async function walkLocaleDir(root, base = "", skipDirs = null) {
  const entries = (await readdir(root, { withFileTypes: true })).filter(
    (e) => !IGNORED.has(e.name)
  );

  const dirs = entries
    .filter((e) => e.isDirectory() && !(skipDirs != null && base === "" && skipDirs.has(e.name)))
    .sort((a, b) => a.name.localeCompare(b.name));

  const mdFiles = entries
    .filter((e) => e.isFile() && extname(e.name) === ".md")
    .sort((a, b) => {
      if (a.name === "README.md") return -1;
      if (b.name === "README.md") return 1;
      return a.name.localeCompare(b.name);
    });

  const txtFiles = entries.filter(
    (e) => e.isFile() && extname(e.name) === ".txt"
  );

  const tree = [];
  const fileMap = {};

  for (const f of mdFiles) {
    const relPath = base ? `${base}/${f.name}` : f.name;
    const content = await readFile(join(root, f.name), "utf-8");
    fileMap[relPath] = content;
    tree.push({ name: f.name, path: relPath, type: "file" });
  }

  for (const f of txtFiles) {
    const relPath = base ? `${base}/${f.name}` : f.name;
    const content = await readFile(join(root, f.name), "utf-8");
    fileMap[relPath] = content;
  }

  for (const d of dirs) {
    const relPath = base ? `${base}/${d.name}` : d.name;
    const sub = await walkLocaleDir(join(root, d.name), relPath, null);
    Object.assign(fileMap, sub.fileMap);
    tree.push({ name: d.name, path: relPath, type: "dir", children: sub.tree });
  }

  return { tree, fileMap };
}

async function copyDirRecursive(srcDir, dstDir) {
  await mkdir(dstDir, { recursive: true });
  const entries = await readdir(srcDir, { withFileTypes: true });
  for (const entry of entries) {
    const srcPath = join(srcDir, entry.name);
    const dstPath = join(dstDir, entry.name);
    if (entry.isDirectory()) {
      await copyDirRecursive(srcPath, dstPath);
    } else if (entry.isFile()) {
      await copyFile(srcPath, dstPath);
    }
  }
}

async function main() {
  const manifest = { locales: {} };

  for (const locale of LOCALES) {
    const { tree, fileMap } = await buildLocale(locale);
    manifest.locales[locale] = { tree, files: fileMap };
  }

  await mkdir(OUT_DIR, { recursive: true });
  await writeFile(OUT_FILE, JSON.stringify(manifest, null, 2) + "\n", "utf-8");

  // Mirror repo-level docs assets into coast-guard/public for web rendering.
  await rm(DOCS_ASSETS_OUT_DIR, { recursive: true, force: true });
  if (await exists(ROOT_ASSETS_DIR)) {
    await copyDirRecursive(ROOT_ASSETS_DIR, DOCS_ASSETS_OUT_DIR);
  }

  const totalFiles = Object.values(manifest.locales).reduce(
    (sum, l) => sum + Object.keys(l.files).length,
    0
  );
  console.log(
    `Generated ${OUT_FILE} — ${LOCALES.length} locales, ${totalFiles} total files`
  );
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
