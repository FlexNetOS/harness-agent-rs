// Differential oracle for WF-18 script-discovery — imports LIVE source.
import { discoverScripts, discoverScriptsForCwd, getDefaultScripts } from './script-discovery';

const ROOT = process.argv[2];
function mapToOrderedArray(m: Map<string, {name:string;path:string;runtime:string}>) {
  // Preserve insertion order; strip ROOT prefix for stable diffing.
  return [...m.entries()].map(([k, v]) => ({ key: k, name: v.name, path: v.path.replaceAll(ROOT, '<ROOT>'), runtime: v.runtime }));
}
async function tryDiscover(dir: string) {
  try { return { ok: true, entries: mapToOrderedArray(await discoverScripts(dir) as any) }; }
  catch (e) { return { ok: false, error: (e as Error).message.replaceAll(ROOT, '<ROOT>') }; }
}
async function tryForCwd(cwd: string) {
  try { return { ok: true, entries: mapToOrderedArray(await discoverScriptsForCwd(cwd) as any) }; }
  catch (e) { return { ok: false, error: (e as Error).message.replaceAll(ROOT, '<ROOT>') }; }
}

(async () => {
  const out: any = {};
  // forCwd: ARCHON_HOME=$ROOT/home is set by caller; repo cwd = $ROOT/repo
  out.for_cwd = await tryForCwd(`${ROOT}/repo`);
  out.repo_scope = await tryDiscover(`${ROOT}/repo/.archon/scripts`);
  out.home_scope = await tryDiscover(`${ROOT}/home/scripts`);
  out.dup = await tryDiscover(`${ROOT}/dup`);
  out.empty = await tryDiscover(`${ROOT}/empty`);
  out.nonexistent = await tryDiscover(`${ROOT}/does_not_exist_xyz`);
  out.unreadable = await tryDiscover(`${ROOT}/unreadable`);
  out.notadir = await tryDiscover(`${ROOT}/notadir_file.ts`);
  out.default_scripts_size = getDefaultScripts().size;
  console.log(JSON.stringify(out, null, 2));
})();
