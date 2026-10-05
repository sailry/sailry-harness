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


if __name__ == '__main__':
    unittest.main()
