import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location('checking', Path(__file__).resolve().parents[1] / 'check-locales.py')
checking = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checking)


class Resources(unittest.TestCase):
    def test_reads_flat_maps_and_folded_prompts(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'en.yml'
            path.write_text('save: "Save"\nprompt: >-\n  Read {name}\n  and report\n')
            self.assertEqual(checking.messages(path), {'save': 'Save', 'prompt': 'Read {name} and report'})

    def test_rejects_duplicate_yaml_and_arb_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            for name, source in [('en.yml', 'save: Save\nsave: Store\n'),
                                  ('app_en.arb', '{"save":"Save", "save":"Store"}')]:
                path = Path(directory) / name
                path.write_text(source)
                with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'Duplicate'):
                    checking.messages(path)

    def test_preserves_key_sets_and_placeholder_counts(self):
        source = {'save': 'Save', 'remove': 'Remove %{name}', 'status': '{count}/{total}'}
        translated = {'save': 'Speichern', 'remove': '%{name} entfernen', 'status': '{count}/{total}'}
        self.assertEqual(checking.compare(source, translated, 'de'), [])
        for changed in ({**translated, 'save': ''},
                        {**translated, 'remove': 'Entfernen'},
                        {**translated, 'remove': '%{name} %{name}'},
                        {**translated, 'remove': '{name} entfernen'},
                        {**translated, 'remove': '% {name} entfernen'},
                        {key: value for key, value in translated.items() if key != 'save'},
                        {**translated, 'extra': 'Extra'}):
            with self.subTest(changed=changed):
                self.assertTrue(checking.compare(source, changed, 'de'))

    def test_inherits_portuguese_base(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'app_pt.arb').write_text('{"save":"Salvar", "remove":"Remover {name}"}')
            (root / 'app_pt_BR.arb').write_text('{"@@locale":"pt_BR"}')
            self.assertEqual(checking.mobile_catalog(root, 'pt-BR'),
                             {'save': 'Salvar', 'remove': 'Remover {name}'})
            (root / 'app_pt_BR.arb').write_text('{"save":"Gravar"}')
            self.assertEqual(checking.mobile_catalog(root, 'pt-BR')['save'], 'Gravar')


class References(unittest.TestCase):
    def test_ignores_comments_and_raw_fixtures(self):
        source = '''
// tr("comment")
let fixture = r#"tr("fixture")"#;
tr("save"); rust_i18n::t!("remove", name = name);
context.tr('status'); tr(dynamic);
'''
        self.assertEqual([key for _, key in checking.references(source)], ['save', 'remove', 'status'])

    def test_language_inventory_matches_codux_reference(self):
        self.assertEqual(set(checking.LOCALES), {'en', 'zh-CN', 'zh-TW', 'ja', 'ko', 'fr', 'de', 'es', 'pt-BR', 'ru'})


class Preview(unittest.TestCase):
    def test_checks_isolated_resources_and_rejects_product_references(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            desktop = root / 'locales'
            mobile = root / 'apps/mobile/lib/l10n'
            preview = root / 'apps/mobile/preview/l10n'
            for path in [desktop, mobile, preview]:
                path.mkdir(parents=True)
            for locale in checking.LOCALES:
                (desktop / f'{locale}.yml').write_text('shared: Label\n')
                name = checking.ARB.get(locale, locale)
                (mobile / f'app_{name}.arb').write_text('{"shared":"Label"}')
                (preview / f'app_{name}.arb').write_text('{"sample":"Fixture"}')
            (mobile / 'app_pt.arb').write_text('{"shared":"Label"}')
            (preview / 'app_pt.arb').write_text('{"sample":"Fixture"}')
            (preview.parent / 'page.dart').write_text("context.tr('sample'); context.tr('shared');")
            self.assertEqual(checking.check(root)[0], [])
            (mobile.parent / 'main.dart').write_text("context.tr('sample');")
            self.assertTrue(any('unknown localization sample' in issue
                                for issue in checking.check(root)[0]))
            (preview / 'app_ja.arb').write_text('{}')
            self.assertTrue(any('Mobile preview ja: missing sample' in issue
                                for issue in checking.check(root)[0]))


class Products(unittest.TestCase):
    def test_desktop_does_not_read_mobile(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            catalogs = root / 'locales'
            catalogs.mkdir()
            for locale in checking.LOCALES:
                (catalogs / f'{locale}.yml').write_text('save: Save\n')
            self.assertEqual(checking.check(root, 'desktop'), ([], 1, 0))

    def test_mobile_does_not_read_desktop(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            catalogs = root / 'apps/mobile/lib/l10n'
            catalogs.mkdir(parents=True)
            for locale in checking.LOCALES:
                name = checking.ARB.get(locale, locale)
                (catalogs / f'app_{name}.arb').write_text('{"save":"Save"}')
            (catalogs / 'app_pt.arb').write_text('{"save":"Save"}')
            self.assertEqual(checking.check(root, 'mobile'), ([], 0, 1))


if __name__ == '__main__':
    unittest.main()
