"""Naming checks operate on source fixtures without changing product behavior."""

import importlib.util
from pathlib import Path
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("check_names", ROOT / "scripts/check-names.py")
checking = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = checking
spec.loader.exec_module(checking)


class Ownership(unittest.TestCase):
    def test_excludes_generated_and_external_source(self):
        for path in ("vendor/framework/file.rs", "plugins/example/main.js",
                     "crates/mobile-bridge/src/frb_generated.rs",
                     "apps/mobile/lib/l10n/generated/app_localizations.dart"):
            with self.subTest(path=path):
                self.assertFalse(checking.owned(Path(path)))
        self.assertTrue(checking.owned(Path("apps/desktop/src/panel/tests.rs")))


class Identifiers(unittest.TestCase):
    def test_counts_words_without_penalizing_domain_terms(self):
        self.assertEqual(checking.words("readHTTPResponse"), ["read", "HTTP", "Response"])
        self.assertEqual(checking.words("authentication_revision"), ["authentication", "revision"])

    def test_rust_excludes_comments_and_raw_fixtures(self):
        source = '''// fn a_name_that_contains_far_too_many_words() {}
const SOURCE: &str = r##"fn a_name_that_contains_far_too_many_words() {}"##;
fn preserves_manual_choice() {}
async fn a_name_that_contains_far_too_many_words() {}
'''
        self.assertEqual(len(checking.violations(source, ".rs")), 1)
        self.assertEqual(checking.violations(source, ".rs")[0][0], 4)

    def test_lifetimes_do_not_hide_inline_functions(self):
        source = "impl<'a> Panel<'a> { fn panel_open(&'a self) {} }"
        result = checking.violations(source, ".rs")
        self.assertEqual(len(result), 1)
        self.assertIn("owning scope 'Panel'", result[0][1])

    def test_python_uses_ast_and_required_discovery_prefix(self):
        source = '''class PanelVisibility:
    def test_preserves_manual_choice(self):
        pass
    def a_name_that_contains_far_too_many_words(self):
        pass
'''
        self.assertEqual(len(checking.violations(source, ".py")), 1)

    def test_dart_typed_methods_and_framework_test_labels(self):
        source = '''Future<Map<String, dynamic>> readCurrentRevision() async {}
Future<void> aNameThatContainsFarTooManyWords() async {}
testWidgets('preserves manual choice', (tester) async {});
// test('one two three four five six seven eight nine ten eleven twelve thirteen', () {});
test(
  'one two three four five six seven eight nine ten eleven twelve thirteen',
  () {},
);
'''
        result = checking.violations(source, ".dart")
        self.assertEqual([line for line, _ in result], [2, 6])

    def test_javascript_functions_and_arrows(self):
        source = '''export async function aNameThatContainsFarTooManyWords() {}
const readRevision = async () => {};
const aNameThatContainsFarTooManyWords = () => {};
'''
        self.assertEqual([line for line, _ in checking.violations(source, ".ts")], [1, 3])

    def test_javascript_calls_are_not_declarations(self):
        source = '''class Documents {
    async open() {
        await documentAction(id, {kind: 'focus'});
        return documentAction(id);
    }
}
'''
        self.assertEqual(checking.violations(source, ".js"), [])

    def test_long_group_context_is_not_a_test_label(self):
        source = '''group('one two three four five six seven eight nine ten eleven twelve thirteen', () {
  test('preserves manual choice', () {});
});
'''
        self.assertEqual(checking.violations(source, ".dart"), [])


class ScopePrefixes(unittest.TestCase):
    def test_file_scope_and_nested_test_modules(self):
        source = '''mod tests {
    fn panel_visibility_preserves_manual_choice() {}
    fn preserves_manual_choice() {}
}
'''
        path = Path("apps/desktop/src/panel_visibility/tests.rs")
        result = checking.violations(source, ".rs", path)
        self.assertEqual([line for line, _ in result], [2])

    def test_type_scope_overrides_implementation_file(self):
        source = '''impl Shell {
    fn header_title(&self) {}
    fn shell_title(&self) {}
}
'''
        path = Path("apps/desktop/src/shell/header.rs")
        result = checking.violations(source, ".rs", path)
        self.assertEqual([line for line, _ in result], [3])

    def test_trait_implementations_keep_required_names(self):
        source = '''impl ListDelegate for Items {
    fn items_count(&self) {}
}
'''
        self.assertEqual(checking.violations(source, ".rs"), [])

    def test_group_labels_supply_context(self):
        source = '''group('Panel visibility', () {
    test('Panel visibility preserves manual choice', () {});
    test('preserves manual choice', () {});
});
'''
        result = checking.violations(source, ".dart")
        self.assertEqual([line for line, _ in result], [2])

    def test_python_class_scope_preserves_discovery_prefix(self):
        source = '''class PanelVisibility:
    def test_panel_visibility_preserves_choice(self):
        pass
    def test_preserves_choice(self):
        pass
'''
        result = checking.violations(source, ".py")
        self.assertEqual([line for line, _ in result], [2])


if __name__ == "__main__":
    unittest.main()
