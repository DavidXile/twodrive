"""Run the existing sharing regression in an installed package debug context.

Launched only by Invoke-CommandInDesktopPackage -PreventBreakaway. This helper
is not included in the application/MSIX payload.
"""
import ctypes
import json
from pathlib import Path
import subprocess
import sys


def main():
    plan = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8-sig'))
    result = {'package_identity_verified': False, 'exit_code': 1}
    try:
        kernel = ctypes.WinDLL('kernel32')
        length = ctypes.c_uint(0)
        assert kernel.GetCurrentPackageFullName(ctypes.byref(length), None) == 122
        value = ctypes.create_unicode_buffer(length.value)
        assert kernel.GetCurrentPackageFullName(ctypes.byref(length), value) == 0
        assert value.value == plan['identity'], 'debug process lost package identity'
        result['package_identity_verified'] = True
        check = subprocess.run([
            sys.executable, plan['script'], '--engine', plan['engine'], '--peer', plan['peer'],
        ], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
        Path(plan['log']).write_bytes(check.stdout)
        result['exit_code'] = check.returncode
    except BaseException as error:
        Path(plan['log']).write_text(repr(error), encoding='utf-8')
    finally:
        Path(plan['result']).write_text(json.dumps(result), encoding='utf-8')


if __name__ == '__main__':
    main()
