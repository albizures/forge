import { openNodeSqliteDatabase } from '@earendil-works/pi-durable/storage/sqlite/node';
import { derivedUrl } from './prepare-readonly.mjs';
const { SqliteStorage } = await import(derivedUrl.href);

// This is an explicit derived adapter, not the upstream open helper. It never
// ignores SQL errors or pretends migration statements succeeded.
export async function openReadOnlyStorage(path, audit = () => {}) {
  const database = await openNodeSqliteDatabase(path, { mode: 'read-only' });
  function reader(executor) {
    return {
      get: (...args) => { audit('get', args[0]); return executor.get(...args); },
      all: (...args) => { audit('all', args[0]); return executor.all(...args); },
      exec: () => { throw new Error('Read-only research adapter: exec forbidden'); },
      run: () => { throw new Error('Read-only research adapter: run forbidden'); },
      runCounted: () => { throw new Error('Read-only research adapter: runCounted forbidden'); },
    };
  }
  const facade = {
    ...reader(database),
    transaction: callback => database.transaction(tx => callback(reader(tx))),
    close: () => database.close(),
  };
  return SqliteStorage.open(facade);
}
