import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseEnv } from 'node:util';

export const projectRoot = fileURLToPath(new URL('..', import.meta.url));

// Read only this project's .env, irrespective of the caller's working directory.
export function projectEnvironment(directory = projectRoot, inherited = process.env) {
  let contents;
  try {
    contents = readFileSync(join(directory, '.env'), 'utf8');
  } catch (error) {
    if (error.code === 'ENOENT') return { ...inherited };
    throw new Error('Could not read the project .env file. Check that it is a readable file.');
  }
  const configured = parseEnv(contents);
  // Windows environment names are case-insensitive, including Path/PATH.
  const names = new Set(Object.keys(configured).map((name) => name.toLowerCase()));
  const base =
    process.platform === 'win32'
      ? Object.fromEntries(
          Object.entries(inherited).filter(([name]) => !names.has(name.toLowerCase())),
        )
      : inherited;
  return { ...base, ...configured };
}
