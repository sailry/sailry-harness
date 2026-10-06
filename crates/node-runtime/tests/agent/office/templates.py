# Exercise installed skill resources with the execution Node's external interpreter.
import json
import subprocess
import sys

package = Path(sys.argv[1])
for name, extension in [('word', 'docx'), ('excel', 'xlsx'), ('powerpoint', 'pptx'), ('pdf', 'pdf')]:
    skill = package / 'skills' / name
    destination = Path('templates') / name / ('sample.' + extension)
    create = [sys.executable, str(skill / 'templates/create.py'), str(destination)]
    subprocess.run(create, check=True)
    inspected = subprocess.run(
        [sys.executable, str(skill / 'scripts/inspect_file.py'), str(destination)],
        capture_output=True, text=True,
    )
    assert inspected.returncode == 0, inspected.stderr
    assert json.loads(inspected.stdout)
    original = destination.read_bytes()
    duplicate = subprocess.run(create, capture_output=True, text=True)
    assert duplicate.returncode != 0 and destination.read_bytes() == original
print('Verified installed Office scripts/templates for all four formats')
