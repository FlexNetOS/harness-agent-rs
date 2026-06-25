// Live TS oracle for WF-09 4a execute_bash_node — replicates the EXACT ladder
// from dag-executor.ts:1580-1675 using Node's real execFile + the REAL source
// formatSubprocessFailure import.
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { formatSubprocessFailure } from '/home/drdave/Desktop/meta/meta-yard/Archon/packages/workflows/src/executor-shared.ts';
const execFileAsync = promisify(execFile);

async function runBash(nodeId: string, script: string, timeout: number, env: Record<string,string>) {
  const subprocessEnv = { ...process.env, ...env };
  try {
    const { stdout, stderr } = await execFileAsync('bash', ['-c', script], { cwd: '/tmp', timeout, env: subprocessEnv });
    const output = stdout.replace(/\n$/, '');
    return { state: 'completed', output, stderrSurfaced: stderr.trim() ? stderr.trim() : null };
  } catch (error: any) {
    const err = error;
    const isTimeout = err.killed === true || (err.message ?? '').includes('timed out');
    const label = `Bash node '${nodeId}'`;
    const formatted = formatSubprocessFailure(err, label);
    let errorMsg: string;
    if (isTimeout) errorMsg = `${label} timed out after ${String(timeout)}ms`;
    else if (err.message?.includes('ENOENT')) errorMsg = `${label} failed: bash executable not found in PATH`;
    else if (err.message?.includes('EACCES')) errorMsg = `${label} failed: permission denied (check cwd permissions)`;
    else errorMsg = formatted.userMessage;
    return { state: 'failed', output: '', error: errorMsg, isTimeout, rawCode: err.code, rawKilled: err.killed };
  }
}

async function main() {
  const R: any = {};
  // Probe 1: newline strip x\n\n -> x\n
  R.p1_double_nl = await runBash('n', "printf 'x\\n\\n'", 30000, {});
  // Probe 2: trailing space kept  "x \n" -> "x "
  R.p2_trailing_space = await runBash('n', "printf 'x \\n'", 30000, {});
  // Probe 3: stderr surface
  R.p3_stderr = await runBash('n', "echo out; echo warnmsg >&2", 30000, {});
  // Probe 4: nonzero exit, NO stderr (the crux)
  R.p4_exit3_nostderr = await runBash('mybash', "exit 3", 30000, {});
  // Probe 5: nonzero exit WITH stderr
  R.p5_exit3_stderr = await runBash('mybash', "echo boom >&2; exit 3", 30000, {});
  // Probe 6: timeout
  R.p6_timeout = await runBash('mybash', "sleep 5", 200, {});
  // Probe 7: env overlay precedence
  R.p7_env = await runBash('n', "echo $ARTIFACTS_DIR-$FOO", 30000, { ARTIFACTS_DIR: 'AD', FOO: 'bar' });
  // Probe 8: empty stdout
  R.p8_empty = await runBash('n', "true", 30000, {});
  console.log(JSON.stringify(R, null, 2));
}
main();
