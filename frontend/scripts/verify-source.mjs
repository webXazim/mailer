import { readFile, readdir, stat } from 'node:fs/promises'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const packageJson = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'))
const dependencyGroups = [packageJson.dependencies ?? {}, packageJson.devDependencies ?? {}]
const floating = dependencyGroups.flatMap(group => Object.entries(group).filter(([, version]) => /latest|\*|^\s*$/.test(version)).map(([name]) => name))
if (floating.length) throw new Error(`Floating dependencies are not allowed: ${floating.join(', ')}`)

const lockJson = JSON.parse(await readFile(new URL('../package-lock.json', import.meta.url), 'utf8'))
const lockRoot = lockJson.packages?.[''] ?? {}
for (const group of ['dependencies', 'devDependencies']) {
  const declared = packageJson[group] ?? {}
  const locked = lockRoot[group] ?? {}
  if (JSON.stringify(declared) !== JSON.stringify(locked)) throw new Error(`${group} in package-lock.json does not match package.json`)
}

for (const asset of ['public/cs-mailer-logo.png', 'public/cs-mailer-32.png', 'public/cs-mailer-192.png', 'public/cs-mailer-512.png']) {
  await stat(new URL(`../${asset}`, import.meta.url))
}

const files = []
async function walk(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) await walk(path)
    else if (/\.(ts|tsx|css|html)$/.test(entry.name)) files.push(path)
  }
}
await walk(join(root, 'src'))
const forbidden = [
  ['document reload', /(?:window\.)?location\.reload\s*\(/],
  ['hard page replacement', /window\.location\.href\s*=/]
]
for (const file of files) {
  const source = await readFile(file, 'utf8')
  for (const [label, pattern] of forbidden) if (pattern.test(source)) throw new Error(`${label} found in ${relative(root, file)}`)
}
console.log(`CS Mailer source verification passed (${files.length} frontend files checked).`)
