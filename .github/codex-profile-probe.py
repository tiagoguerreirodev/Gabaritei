import base64
import json
import os
import pathlib
import platform
import socket
import subprocess
import tempfile

windows = platform.system() == 'Windows'
print(platform.platform(), platform.machine(), flush=True)
packages = pathlib.Path('probe-cli/node_modules/@openai')
binary_name = 'codex.exe' if windows else 'codex'
binaries = [
    p.resolve() for p in packages.rglob(binary_name)
    if p.is_file() and 'vendor' in p.parts
]
assert len(binaries) == 1, binaries
binary, = binaries
subprocess.run([str(binary), '--version'], check=True)

with tempfile.TemporaryDirectory(prefix='gabaritei-restricted-') as temp:
    base = pathlib.Path(temp).resolve()
    work = base / 'run'
    work.mkdir()
    attachment = work / 'attachment.txt'
    attachment.write_text('synthetic', encoding='utf-8')
    outside = base / 'outside.txt'
    outside.write_text('synthetic', encoding='utf-8')
    schema = work / 'invalid.json'
    schema.write_text('}', encoding='utf-8')
    codex_home = base / 'codex-home'
    codex_home.mkdir()
    runtime = binary.parent.parent
    install = runtime if runtime.joinpath('codex-package.json').is_file() else binary
    filesystem = (
        'permissions.gabaritei.filesystem={":minimal"="read", '
        + json.dumps(str(install), ensure_ascii=False) + '="read", '
        + json.dumps(str(work), ensure_ascii=False) + '="read"}'
    )
    config = ['-c', filesystem, '-c', 'permissions.gabaritei.network.enabled=false']
    env = {key: value for key, value in os.environ.items() if key not in (
        'OPENAI_API_KEY', 'CODEX_API_KEY', 'ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN',
    )}
    env['CODEX_HOME'] = str(codex_home)

    def run(cli_args, timeout=45):
        result = subprocess.run(
            [str(binary), *cli_args], cwd=work, env=env, timeout=timeout,
            stdin=subprocess.DEVNULL, capture_output=True, text=True, errors='replace',
        )
        print(result.stdout + result.stderr, flush=True)
        return result

    with socket.create_server(('127.0.0.1', 0)) as listener:
        port = listener.getsockname()[1]
        args = ['sandbox', '-P', 'gabaritei', '-C', str(work), *config]
        if windows:
            probe = r'''
$ErrorActionPreference = 'Stop'
function CanRead($path) {
    try { $null = [System.IO.File]::ReadAllText($path); $true } catch { $false }
}
$cwd = (Get-Location).Path
Write-Output ('attachment=' + (CanRead (Join-Path $cwd 'attachment.txt')))
Write-Output ('outside=' + (CanRead (Join-Path (Split-Path $cwd) 'outside.txt')))
try { [System.IO.File]::WriteAllText((Join-Path $cwd 'probe-write.txt'), 'synthetic'); Write-Output 'write=True' } catch { Write-Output 'write=False' }
try { [System.Net.Sockets.TcpClient]::new('127.0.0.1', PORT).Close(); Write-Output 'network=True' } catch { Write-Output 'network=False' }
foreach ($path in @($env:windir, $env:ProgramFiles, $env:USERPROFILE, $env:TEMP)) {
    try { $null = Get-ChildItem -LiteralPath $path; Write-Output "root=$path,read=True" } catch { Write-Output "root=$path,read=False" }
}
'''.replace('PORT', str(port))
            encoded = base64.b64encode(probe.encode('utf-16-le')).decode('ascii')
            child = ['powershell.exe', '-NoProfile', '-NonInteractive', '-EncodedCommand', encoded]
            unsupported = run([*args, '-c', 'windows.sandbox="unelevated"', '--', *child])
            assert unsupported.returncode != 0
            assert 'requires the elevated Windows sandbox backend' in unsupported.stderr
            args += ['-c', 'windows.sandbox="elevated"']
        else:
            probe = r'''
import os
import pathlib
import socket
import sys
print('attachment=' + str(pathlib.Path('attachment.txt').read_text() == 'synthetic'))
try:
    pathlib.Path.cwd().parent.joinpath('outside.txt').read_text()
    print('outside=True')
except OSError:
    print('outside=False')
try:
    pathlib.Path('probe-write.txt').write_text('synthetic')
    print('write=True')
except OSError:
    print('write=False')
try:
    with socket.create_connection(('127.0.0.1', int(sys.argv[1])), timeout=1):
        print('network=True')
except OSError:
    print('network=False')
for path in ('/System', '/Library', '/usr', '/dev', '/private/tmp', '/Users', os.environ['HOME']):
    print(f'root={path},read={os.access(path, os.R_OK)}')
'''
            child = ['/usr/bin/python3', '-c', probe, str(port)]
        check = run([*args, '--', *child])

    strict = [
        'exec', '--strict-config', '--ignore-user-config', '--ignore-rules', '--ephemeral',
        '--skip-git-repo-check', '--output-schema', str(schema),
        '-c', 'default_permissions="gabaritei"', '-c', 'approval_policy="never"', *config,
        '-c', 'shell_environment_policy.inherit="core"',
    ]
    accepted = run([*strict, 'Synthetic'])
    assert 'is not valid JSON' in accepted.stderr, accepted.returncode
    rejected = run([*strict, '-c', 'gabaritei_unknown_key=true', 'Synthetic'])
    assert 'unknown configuration field' in rejected.stderr, rejected.returncode
    if windows:
        assert check.returncode != 0, check.returncode
        assert 'requires effective `:root` read access' in check.stderr
    else:
        assert check.returncode == 0, check.returncode
        answers = set(check.stdout.splitlines())
        assert {'attachment=True', 'outside=False', 'write=False', 'network=False'} <= answers
