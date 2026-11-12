import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/providers/api.dart' as api;
import 'package:sailry_mobile/features/settings/providers/controller.dart';
import 'package:sailry_mobile/features/settings/providers/rules.dart';

Json provider({List<Json> models = const [], String fallback = ''}) => {
  'id': 'p1',
  'revision': 4,
  'preset': {'id': 'openai'},
  'binding': {'package': 'kept'},
  'oauth': {'issuer': 'kept'},
  'consent': {'scope': 'kept'},
  'name': 'gpt',
  'api': 'responses',
  'authentication': 'api_key',
  'endpoint': 'https://api.example.com/v1',
  'enabled': true,
  'models': models,
  'default_model': fallback,
  'credential': {'node': 'n', 'id': 'c'},
  'future_field': 1,
};

Json model(String id, {bool reasoning = false}) => {
  'id': id,
  'context': 128000,
  'output': 16384,
  'vision': false,
  'tools': true,
  'reasoning': reasoning,
  'generates': [],
  'efforts': [],
  'custom_efforts': false,
  'default_effort': 'default',
};

void main() {
  group('labels', () {
    test("formats endpoints models and token counts", () {
      for (final (actual, expected, reason) in <(Object?, Object?, String)>[
        (
          endpointHost('https://api.deepseek.com/v1'),
          'api.deepseek.com',
          "endpoint host keeps ports",
        ),
        (
          endpointHost('http://localhost:11434'),
          'localhost:11434',
          "endpoint host keeps ports",
        ),
        (endpointHost('not a url'), 'not a url', "endpoint host keeps ports"),
        (tokenLabel(400000), '400K', "token counts are compact"),
        (tokenLabel(1000000), '1M', "token counts are compact"),
        (tokenLabel(1500000), '1.5M', "token counts are compact"),
        (tokenLabel(512), '512', "token counts are compact"),
        (
          modelLabel({'id': 'gpt-5'}),
          'gpt-5',
          "model label falls back to the id",
        ),
        (
          modelLabel({'id': 'gpt-5', 'display_name': 'GPT-5'}),
          'GPT-5',
          "model label falls back to the id",
        ),
        (parseTokens('4096'), 4096, "tokens must be positive integers"),
        (parseTokens('0'), isNull, "tokens must be positive integers"),
        (parseTokens('-1'), isNull, "tokens must be positive integers"),
        (parseTokens('x'), isNull, "tokens must be positive integers"),
      ]) {
        expect(actual, expected, reason: reason);
      }
    });
  });

  group('saving', () {
    test("keeps revisions and unknown provider fields", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "request keeps unknown fields and the edited revision",
          (caseName) {
            final edited = {...provider(), 'name': 'renamed'};
            final request = saveRequest(edited, secret: 'sk-new');
            expect(request['expected_revision'], 4, reason: caseName);
            expect(request['secret'], 'sk-new', reason: caseName);
            final saved = request['provider'] as Json;
            for (final field in [
              'preset',
              'binding',
              'oauth',
              'consent',
              'credential',
              'future_field',
            ]) {
              expect(
                saved[field],
                provider()[field],
                reason: '$caseName: $field',
              );
            }
            expect(saved['name'], 'renamed', reason: caseName);
          },
        ),
        (
          "new providers start at revision zero without models",
          (caseName) {
            final created = newProvider(
              id: 'p2',
              name: 'Local',
              api: 'chat_completions',
              endpoint: 'http://localhost:11434',
            );
            expect(
              saveRequest(created)['expected_revision'],
              0,
              reason: caseName,
            );
            expect(created['models'], isEmpty, reason: caseName);
            expect(created['default_model'], '', reason: caseName);
            expect(created['authentication'], 'api_key', reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });

    test("resolves provider endpoint catalogs", () {
      for (final (actual, expected, reason) in <(Object?, Object?, String)>[
        (
          api.fixedEndpoint('open_code_go'),
          'https://opencode.ai/zen/go/v1',
          "fixed endpoints follow their API",
        ),
        (
          api.fixedEndpoint('responses'),
          isNull,
          "fixed endpoints follow their API",
        ),
        (
          catalogProvider('https://api.openai.com/v1/'),
          'openai',
          "catalog provider matches well-known endpoints",
        ),
        (
          catalogProvider('https://api.example.com'),
          isNull,
          "catalog provider matches well-known endpoints",
        ),
      ]) {
        expect(actual, expected, reason: reason);
      }
    });
  });

  group('models', () {
    test("maintains model defaults through edits", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "default follows removal and the first addition",
          (caseName) {
            final empty = provider();
            final added = withModels(empty, [model('a'), model('b')]);
            expect(added['default_model'], 'a', reason: caseName);
            final picked = {...added, 'default_model': 'b'};
            expect(
              removeModel(picked, 'b')['default_model'],
              'a',
              reason: caseName,
            );
            expect(
              removeModel(removeModel(picked, 'a'), 'b')['default_model'],
              '',
              reason: caseName,
            );
          },
        ),
        (
          "renaming moves the default and drops reported metadata",
          (caseName) {
            final current = provider(
              models: [
                {...model('a'), 'display_name': 'A', 'capabilities': {}},
              ],
              fallback: 'a',
            );
            final renamed = renameModel(findModel(current, 'a')!, 'b');
            expect(
              renamed.containsKey('display_name'),
              isFalse,
              reason: caseName,
            );
            expect(
              renamed.containsKey('capabilities'),
              isFalse,
              reason: caseName,
            );
            final next = replaceModel(current, 'a', renamed);
            expect(next['default_model'], 'b', reason: caseName);
            expect(models(next).single['id'], 'b', reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });

    test('capability switches are explicit metadata', () {
      final edited = setCapability(
        {
          ...model('a'),
          'capabilities': {'tools': true},
        },
        'vision',
        true,
      );
      expect(edited['vision'], isTrue);
      expect(edited['capabilities'], {'tools': true, 'vision': true});
    });

    test('limits reject output above context', () {
      expect(limitsProblem(model('a')), isNull);
      expect(
        limitsProblem({...model('a'), 'output': 200000}),
        'providerLimitsInvalid',
      );
      expect(limitsProblem({...model('a'), 'context': 0}), isNotNull);
    });
  });

  group('discovery', () {
    test("resolves discovery metadata and merges models", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "missing metadata uses protocol defaults",
          (caseName) {
            final configured = configure({'id': 'x'});
            expect(configured['context'], 200000, reason: caseName);
            expect(configured['output'], 16384, reason: caseName);
            expect(configured['vision'], isTrue, reason: caseName);
            expect(configured['tools'], isTrue, reason: caseName);
            expect(configured['reasoning'], isFalse, reason: caseName);
            expect(configured['default_effort'], 'default', reason: caseName);
            expect(
              configured.containsKey('capabilities'),
              isFalse,
              reason: caseName,
            );
          },
        ),
        (
          "reported metadata wins over the catalog",
          (caseName) {
            final configured = configure(
              {
                'id': 'x',
                'display_name': 'X',
                'context': 400000,
                'capabilities': {
                  'vision': false,
                  'reasoning': true,
                  'efforts': ['default', 'low', 'high'],
                  'default_effort': 'high',
                },
              },
              {
                'context': 1000000,
                'output': 128000,
                'inputs': ['text', 'image'],
                'outputs': ['text', 'image'],
                'tools': false,
              },
            );
            expect(configured['display_name'], 'X', reason: caseName);
            expect(configured['context'], 400000, reason: caseName);
            expect(configured['output'], 128000, reason: caseName);
            expect(configured['vision'], isFalse, reason: caseName);
            expect(configured['tools'], isFalse, reason: caseName);
            expect(configured['reasoning'], isTrue, reason: caseName);
            expect(configured['efforts'], ['low', 'high'], reason: caseName);
            expect(configured['default_effort'], 'high', reason: caseName);
            expect(configured['generates'], ['image'], reason: caseName);
          },
        ),
        (
          "catalog inputs without images disable vision",
          (caseName) {
            expect(
              configure(
                {'id': 'x'},
                {
                  'inputs': ['text'],
                },
              )['vision'],
              isFalse,
              reason: caseName,
            );
          },
        ),
        (
          "merging skips models the provider has",
          (caseName) {
            final current = provider(models: [model('a')], fallback: 'a');
            final merged = mergeDiscovered(current, [
              {'id': 'a', 'context': 1},
              {'id': 'b', 'context': 64000, 'output': 8000},
            ], {});
            expect(models(merged).map((model) => model['id']), [
              'a',
              'b',
            ], reason: caseName);
            expect(findModel(merged, 'a'), model('a'), reason: caseName);
            expect(findModel(merged, 'b')!['context'], 64000, reason: caseName);
            expect(merged['default_model'], 'a', reason: caseName);
            expect(merged['future_field'], 1, reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });

  group('efforts', () {
    test('parses names and budgets like Desktop', () {
      expect(parseEffort(' xhigh '), 'xhigh');
      expect(parseEffort('dynamic'), {'budget': -1});
      expect(parseEffort('4096'), {'budget': 4096});
      expect(parseEffort('-1'), {'budget': -1});
      for (final value in [
        'default',
        'High',
        '',
        '0x10',
        '1.5',
        '2147483648',
      ]) {
        expect(parseEffort(value), isNull, reason: value);
      }
    });

    test("formats and selects declared efforts", () {
      for (final (actual, expected, reason) in <(Object?, Object?, String)>[
        (effortValue('xhigh'), 'xhigh', "values stay raw"),
        (effortValue({'budget': 4096}), '4096', "values stay raw"),
        (effortValue({'budget': -1}), '-1', "values stay raw"),
        (
          initialEffort(['low', 'high'], 'high'),
          'high',
          "initial effort prefers a listed choice",
        ),
        (
          initialEffort(['low', 'high'], 'max'),
          'low',
          "initial effort prefers a listed choice",
        ),
        (
          initialEffort([], 'high'),
          'default',
          "initial effort prefers a listed choice",
        ),
        (
          initialEffort(
            [
              {'budget': 2048},
            ],
            {'budget': 2048},
          ),
          {'budget': 2048},
          "initial effort prefers a listed choice",
        ),
      ]) {
        expect(actual, expected, reason: reason);
      }
    });

    test("updates declared efforts and defaults", () {
      for (final (caseName, run) in <(String, void Function(String))>[
        (
          "adding appends and defaults the first effort",
          (caseName) {
            var edited = addEffort(model('a', reasoning: true), 'high')!;
            expect(edited['efforts'], ['high'], reason: caseName);
            expect(edited['default_effort'], 'high', reason: caseName);
            expect(edited['custom_efforts'], isTrue, reason: caseName);
            edited = addEffort(edited, '4096')!;
            expect(edited['efforts'], [
              'high',
              {'budget': 4096},
            ], reason: caseName);
            expect(edited['default_effort'], 'high', reason: caseName);
          },
        ),
        (
          "adding rejects invalid, listed and excess efforts",
          (caseName) {
            final listed = {
              ...model('a'),
              'efforts': [
                'low',
                {'budget': 4096},
              ],
            };
            expect(addEffort(listed, 'nope'), isNull, reason: caseName);
            expect(addEffort(listed, 'low'), isNull, reason: caseName);
            expect(addEffort(listed, '4096'), isNull, reason: caseName);
            final full = {
              ...model('a'),
              'efforts': [
                for (var i = 1; i <= maxEfforts; i++) {'budget': i},
              ],
            };
            expect(addEffort(full, 'low'), isNull, reason: caseName);
          },
        ),
        (
          "removing the default falls back to the first effort",
          (caseName) {
            var edited = removeEffort({
              ...model('a'),
              'efforts': ['low', 'medium', 'high'],
              'default_effort': 'medium',
            }, 1);
            expect(edited['efforts'], ['low', 'high'], reason: caseName);
            expect(edited['default_effort'], 'low', reason: caseName);
            expect(edited['custom_efforts'], isTrue, reason: caseName);
            edited = removeEffort(edited, 1);
            expect(edited['default_effort'], 'low', reason: caseName);
            edited = removeEffort(edited, 0);
            expect(edited['efforts'], isEmpty, reason: caseName);
            expect(edited['default_effort'], 'default', reason: caseName);
          },
        ),
        (
          "default choice is explicit",
          (caseName) {
            final edited = setDefaultEffort({
              ...model('a'),
              'efforts': ['low', 'high'],
            }, 'low');
            expect(edited['default_effort'], 'low', reason: caseName);
            expect(edited['custom_efforts'], isTrue, reason: caseName);
          },
        ),
      ]) {
        run(caseName);
      }
    });
  });
}
