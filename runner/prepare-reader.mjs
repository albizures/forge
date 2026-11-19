// Maintained derived reader over pi-durable 1.5.2 (MIT, Earendil Works).
// Read/decoding implementation remains upstream's; mutation and migration paths
// are disabled explicitly. See README.md for provenance/upgrade obligations.
import { readFile, writeFile, rename } from 'node:fs/promises';
import { createHash } from 'node:crypto';
const destination = new URL('./forge-readonly-storage.js', import.meta.resolve('@earendil-works/pi-durable/storage/sqlite/node'));
const original = await readFile(new URL('./storage.js', destination), 'utf8');
if (createHash('sha256').update(original).digest('hex') !== '0d9d8cf56898afe5d38f0e56bd3cbcad23a8e68b942b81c39587ac731fe48642') {
  throw new Error('reader-base-incompatible: expected inspected pi-durable 1.5.2');
}
function replace(text, before, after) {
  if (text.split(before).length !== 2) throw new Error('reader-source-incompatible');
  return text.replace(before, after);
}
let derived = replace(original,
  'import { applySqliteMigrationsForSchema } from "./migrations.js";',
  'import { CURRENT_SQLITE_SCHEMA_VERSION } from "./migrations.js";');
derived = replace(derived,
  '            await applySqliteMigrationsForSchema(db, schema);',
  `            const version = await db.get(\`SELECT version FROM \${schema.tables.durableSchema} WHERE singleton = 1\`);
            if (version?.version !== CURRENT_SQLITE_SCHEMA_VERSION)
                throw new Error('reader-schema-incompatible');`);
derived = replace(derived, '    async commit(batches, receive, _context) {',
  `    async commit(batches, receive, _context) {
        throw new Error('reader-mutation-forbidden');`);
derived = replace(derived, '    async mintId() {',
  `    async mintId() {
        throw new Error('reader-mutation-forbidden');`);
const pending = new URL(`./forge-readonly-storage.${process.pid}.tmp`, destination);
await writeFile(pending, '// Derived from pi-durable 1.5.2, MIT; see Forge runner/README.md\n' + derived, { mode: 0o600 });
await rename(pending, destination);
console.log('reader sha256:', createHash('sha256').update(await readFile(destination)).digest('hex'));
