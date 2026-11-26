// What the machine is, read outside the page, for results.json: the system, the GPU as the system names it (WebKit
// answers the page a fixed "Apple GPU"), whether this is WSL or a virtual machine, and the commit measured. No user
// name, host name or path goes in: the file is sent back from other machines.
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import os from 'node:os';

const run = (cmd, args, options = {}) => {
  const r = spawnSync(cmd, args, { encoding: 'utf8', timeout: 20000, ...options });
  return r.status === 0 ? r.stdout.trim() : null;
};

/** WSL, from /proc/version and the variables WSL sets: Microsoft's kernel names itself there. */
export function isWsl(procVersion, env) {
  return /microsoft|wsl/i.test(procVersion ?? '') || Boolean(env.WSL_DISTRO_NAME || env.WSL_INTEROP);
}

/** What systemd-detect-virt printed: the hypervisor, or "none" on bare metal, where it exits with 1. Both are an
 * answer, so the exit status does not count; no output at all (no such command) is unknown. */
export function virtualOf(stdout) {
  return (stdout ?? '').trim().split('\n')[0].trim() || 'unknown';
}

/** The lines of `glxinfo -B` that say what rasterises: the renderer and whether it is accelerated. */
export function glxSummary(text) {
  const line = (key) => (text ?? '').split('\n').find((l) => l.trim().startsWith(key))?.split(':').slice(1).join(':').trim() ?? null;
  return { renderer: line('OpenGL renderer string'), accelerated: line('Accelerated'), vendor: line('OpenGL vendor string') };
}

function linux() {
  const procVersion = existsSync('/proc/version') ? readFileSync('/proc/version', 'utf8') : '';
  const glxinfo = run('bash', ['-c', 'glxinfo -B 2>&1 | head -40']);
  const release = existsSync('/etc/os-release') ? /PRETTY_NAME="([^"]*)"/.exec(readFileSync('/etc/os-release', 'utf8'))?.[1] : null;
  return {
    system: release ?? `Linux ${os.release()}`,
    wsl: isWsl(procVersion, process.env),
    wslg: existsSync('/mnt/wslg'),
    virtual: virtualOf(spawnSync('systemd-detect-virt', [], { encoding: 'utf8', timeout: 20000 }).stdout),
    gpu: glxSummary(glxinfo),
    glxinfo,
  };
}

function macos() {
  const displays = run('system_profiler', ['SPDisplaysDataType']) ?? '';
  const chips = displays.split('\n').filter((l) => /Chipset Model:/.test(l)).map((l) => l.split(':').slice(1).join(':').trim());
  return {
    system: `macOS ${run('sw_vers', ['-productVersion']) ?? os.release()}`,
    wsl: false,
    virtual: run('sysctl', ['-n', 'kern.hv_vmm_present']) === '1' ? 'yes' : 'no',
    gpu: { renderer: chips.join('; ') || null },
  };
}

function windows() {
  const ps = (command) => run('powershell', ['-NoProfile', '-NonInteractive', '-Command', command]);
  const gpus = ps('(Get-CimInstance Win32_VideoController).Name') ?? '';
  const model = ps('(Get-CimInstance Win32_ComputerSystem).Model') ?? '';
  const caption = ps('(Get-CimInstance Win32_OperatingSystem).Caption') ?? 'Windows';
  return {
    system: `${caption} ${os.release()}`,
    wsl: false,
    virtual: /virtual|vmware|kvm|hvm/i.test(model) ? 'yes' : model ? 'no' : 'unknown',
    gpu: { renderer: gpus.split(/\r?\n/).map((s) => s.trim()).filter(Boolean).join('; ') || null },
  };
}

export function describeMachine(repo) {
  const own = process.platform === 'linux' ? linux() : process.platform === 'darwin' ? macos() : windows();
  const commit = run('git', ['rev-parse', 'HEAD'], { cwd: repo });
  const dirty = run('git', ['status', '--porcelain', '--untracked-files=no'], { cwd: repo });
  return {
    platform: process.platform,
    arch: os.arch(),
    cpu: os.cpus()[0]?.model?.trim() ?? null,
    cpus: os.cpus().length,
    memory_gb: Math.round((os.totalmem() / 2 ** 30) * 10) / 10,
    node: process.version,
    commit: commit ? { sha: commit, clean: dirty === '' } : null,
    ...own,
  };
}
