#!/usr/bin/env python3
"""Check Sailry translation coverage, placeholders and literal resource references."""

import argparse
from collections import Counter
import json
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
LOCALES = ('en', 'zh-CN', 'zh-TW', 'ja', 'ko', 'fr', 'de', 'es', 'pt-BR', 'ru')
ARB = {'zh-CN': 'zh', 'zh-TW': 'zh_Hant', 'pt-BR': 'pt_BR'}
PLACEHOLDER = re.compile(r'%?\{[A-Za-z_]\w*\}')
TOKENS = re.compile(
    r'//[^\n]*|/\*.*?\*/|'
    r'\b(?:br|r)(?P<hashes>\#{0,16})".*?"(?P=hashes)|'
    r"[rR]?(?:'''(?:.*?)'''|\"\"\"(?:.*?)\"\"\")|"
    r'"(?:\\.|[^"\\])*"|'
    r"'(?:\\.|[^'\\\n])*'",
    re.DOTALL,
)
CALL = re.compile(r'\b(?:tr|t!)\s*\(')


def pairs(items):
    result = {}
    for key, value in items:
        if key in result:
            raise ValueError(f'Duplicate translation key: {key}')
        result[key] = value
    return result


def messages(path):
    if path.suffix == '.arb':
        data = json.loads(path.read_text(), object_pairs_hook=pairs)
        return {key: value for key, value in data.items() if not key.startswith('@')}
    rows = []
    lines = path.read_text().splitlines()
    index = 0
    # Sailry's YAML catalogs are flat string maps, including folded prompt text.
    while index < len(lines):
        line = lines[index]
        index += 1
        if not line.strip() or line.startswith('#'):
            continue
        match = re.fullmatch(r'([A-Za-z_]\w*):\s*(.*)', line)
        if match is None:
            raise ValueError(f'Expected a flat translation map: {path}:{index}')
        key, value = match.groups()
        if value in ('>', '>-', '|', '|-'):
            block = []
            while index < len(lines) and (lines[index].startswith('  ') or not lines[index].strip()):
                block.append(lines[index].strip())
                index += 1
            value = ('\n' if value.startswith('|') else ' ').join(block).rstrip()
        elif value.startswith('"'):
            value = json.loads(value)
        elif value.startswith("'") and value.endswith("'"):
            value = value[1:-1].replace("''", "'")
        rows.append((key, value))
    return pairs(rows)


def catalogs(root, locale):
    paths = sorted((root / 'locales').glob(f'*.{locale}.yml'))
    plain = root / 'locales' / f'{locale}.yml'
    if plain.exists():
        paths.append(plain)
    if not paths:
        raise ValueError(f'Missing desktop language: {locale}')
    return pairs(item for path in paths for item in messages(path).items())


def mobile_catalog(root, locale):
    paths = [root / f'app_{ARB.get(locale, locale)}.arb']
    if locale == 'pt-BR':
        # Flutter's regional class inherits the base catalog's translations.
        paths.insert(0, root / 'app_pt.arb')
    result = {}
    for path in paths:
        result.update(messages(path))
    return result


def compare(source, translated, name):
    issues = []
    for key in sorted(source.keys() - translated.keys()):
        issues.append(f'{name}: missing {key}')
    for key in sorted(translated.keys() - source.keys()):
        issues.append(f'{name}: unknown {key}')
    for key in sorted(source.keys() & translated.keys()):
        value = translated[key]
        if not isinstance(value, str) or not value.strip():
            issues.append(f'{name}: empty or non-string {key}')
        elif Counter(PLACEHOLDER.findall(source[key])) != Counter(PLACEHOLDER.findall(value)):
            issues.append(f'{name}: placeholders differ for {key}')
    return issues


def references(source):
    literals = {}
    def mask(match):
        literal = match.group()
        literals[match.start()] = literal
        return re.sub(r'[^\n]', ' ', literal)
    code = TOKENS.sub(mask, source)
    for call in CALL.finditer(code):
        for offset, literal in literals.items():
            if offset < call.end():
                continue
            if code[call.end():offset].strip():
                break
            match = re.fullmatch(r"[\"']([A-Za-z_]\w*)[\"']", literal)
            if match:
                yield source.count('\n', 0, offset) + 1, match.group(1)
            break


def check(root, product='all'):
    issues = []
    desktop = catalogs(root, 'en') if product != 'mobile' else {}
    mobile_root = root / 'apps/mobile/lib/l10n'
    mobile = messages(mobile_root / 'app_en.arb') if product != 'desktop' else {}
    for locale in LOCALES:
        if product != 'mobile':
            issues.extend(compare(desktop, catalogs(root, locale), f'Desktop {locale}'))
        if product != 'desktop':
            path = mobile_root / f'app_{ARB.get(locale, locale)}.arb'
            if not path.exists():
                issues.append(f'Mobile {locale}: missing resource file')
            else:
                issues.extend(compare(mobile, mobile_catalog(mobile_root, locale), f'Mobile {locale}'))
    sources = []
    if product != 'mobile':
        sources.append((root / 'apps/desktop/src', '.rs', desktop))
    if product != 'desktop':
        sources.append((root / 'apps/mobile/lib', '.dart', mobile))
    for directory, suffix, keys in sources:
        for path in sorted(directory.rglob(f'*{suffix}')):
            if 'generated' in path.parts:
                continue
            for line, key in references(path.read_text()):
                if key not in keys:
                    issues.append(f'{path.relative_to(root)}:{line}: unknown localization {key}')
    preview_root = root / 'apps/mobile/preview'
    if product != 'desktop' and preview_root.exists():
        preview = messages(preview_root / 'l10n/app_en.arb')
        for locale in LOCALES:
            issues.extend(compare(preview, mobile_catalog(preview_root / 'l10n', locale),
                                  f'Mobile preview {locale}'))
        for path in sorted(preview_root.rglob('*.dart')):
            if 'generated' in path.parts:
                continue
            for line, key in references(path.read_text()):
                if key not in mobile and key not in preview:
                    issues.append(f'{path.relative_to(root)}:{line}: unknown localization {key}')
    return issues, len(desktop), len(mobile)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--product', choices=('all', 'desktop', 'mobile'), default='all')
    args = parser.parse_args()
    try:
        issues, desktop, mobile = check(ROOT, args.product)
    except (ValueError, OSError) as error:
        print(error, file=sys.stderr)
        return 1
    if issues:
        print('\n'.join(issues), file=sys.stderr)
        return 1
    print(f'{len(LOCALES)} languages checked: {desktop} desktop and {mobile} mobile keys')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
