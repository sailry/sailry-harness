import json
import os
import pathlib
import subprocess
import sys

manager = pathlib.Path(sys.argv[0]).name
arguments = sys.argv[1:]
binary = "#!/bin/sh\nprintf 'ready\\n'\n"

def install_binary(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(binary)
    path.chmod(0o755)

if manager == "npm":
    prefix = pathlib.Path(arguments[arguments.index("--prefix") + 1])
    assert "fixture@1.0" in arguments
    assert "--no-audit" in arguments
    install_binary(prefix / "node_modules/.bin/fixture")
elif manager == "cargo":
    prefix = pathlib.Path(arguments[arguments.index("--root") + 1])
    assert arguments[-2:] == ["--version", "1.0"]
    assert "--locked" in arguments
    install_binary(prefix / "bin/fixture")
elif manager == "go":
    assert arguments == ["install", "example.invalid/fixture@1.0"]
    install_binary(pathlib.Path(os.environ["GOBIN"]) / "fixture")
elif manager == "python3":
    assert arguments[:2] == ["-m", "venv"]
    venv = pathlib.Path(arguments[2])
    subprocess.run([sys.executable, "-m", "venv", "--without-pip", str(venv)], check=True)
    version = f"python{sys.version_info.major}.{sys.version_info.minor}"
    site = venv / "lib" / version / "site-packages"
    (site / "pip.py").write_text('''import pathlib,sys
assert sys.argv[1:] == ['install','--disable-pip-version-check','--no-input','fixture==1.0']
site=pathlib.Path(__file__).parent
(site/'fixture.py').write_text("def main():\\n    print('ready')\\n")
metadata=site/'fixture-1.0.dist-info'
metadata.mkdir()
(metadata/'METADATA').write_text('Name: fixture\\nVersion: 1.0\\n')
(metadata/'entry_points.txt').write_text('[console_scripts]\\nfixture = fixture:main\\n')
binary=pathlib.Path(sys.executable).parent/'fixture'
binary.write_text('#!'+sys.executable+'\\nfrom fixture import main\\nmain()\\n')
binary.chmod(0o755)
''')
else:
    raise AssertionError(manager)
