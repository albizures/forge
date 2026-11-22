// Research-only derived module, NOT a production dependency patch. The stock
// module stays untouched so tests can compare both paths in the same process.
import { readFile, writeFile, rename } from 'node:fs/promises';
import { createHash } from 'node:crypto';
export const derivedUrl = new URL('./forge-upstream-probe-storage.js', import.meta.resolve('@earendil-works/pi-durable/storage/sqlite/node'));
const originalUrl = new URL('./storage.js', derivedUrl);
const original = await readFile(originalUrl, 'utf8');
if (createHash('sha256').update(original).digest('hex') !== '0d9d8cf56898afe5d38f0e56bd3cbcad23a8e68b942b81c39587ac731fe48642') {
  throw new Error('Research read-only adapter requires the inspected pi-durable 1.5.2 module');
}
function replaceOnce(text, before, after) {
  if (text.split(before).length !== 2) throw new Error('Read-only research patch no longer applies uniquely');
  return text.replace(before, after);
}
let derived = replaceOnce(original,
  'import { applySqliteMigrationsForSchema } from "./migrations.js";',
  'import { CURRENT_SQLITE_SCHEMA_VERSION } from "./migrations.js";');
derived = replaceOnce(derived,
  '            await applySqliteMigrationsForSchema(db, schema);',
  `            // Never initialize or migrate, even an empty or older database.
            const version = await db.get(\`SELECT version FROM \${schema.tables.durableSchema} WHERE singleton = 1\`);
            if (version?.version !== CURRENT_SQLITE_SCHEMA_VERSION)
                throw new Error('Read-only research adapter: incompatible schema');`);
derived = replaceOnce(derived,
  '    async commit(batches, receive, _context) {',
  `    async commit(batches, receive, _context) {
        throw new Error('Read-only research adapter: commit forbidden');
        /* istanbul ignore next: inherited implementation is unreachable */`);
derived = replaceOnce(derived,
  '    async mintId() {',
  `    async mintId() {
        throw new Error('Read-only research adapter: mintId forbidden');`);
// The published read implementations and their ordering/decoding stay unchanged.
// This generated sibling uses the same relative imports as the inspected module.
const pendingUrl = new URL(`./forge-upstream-probe-storage.${process.pid}.tmp`, derivedUrl);
await writeFile(pendingUrl, '// GENERATED research-only read adapter; see prepare-readonly.mjs\n' + derived);
await rename(pendingUrl, derivedUrl);
