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
    test('endpoint host keeps ports', () {
      expect(endpointHost('https://api.deepseek.com/v1'), 'api.deepseek.com');
      expect(endpointHost('http://localhost:11434'), 'localhost:11434');
      expect(endpointHost('not a url'), 'not a url');
    });

    test('token counts are compact', () {
      expect(tokenLabel(400000), '400K');
      expect(tokenLabel(1000000), '1M');
      expect(tokenLabel(1500000), '1.5M');
      expect(tokenLabel(512), '512');
    });

    test('model label falls back to the id', () {
      expect(modelLabel({'id': 'gpt-5'}), 'gpt-5');
      expect(modelLabel({'id': 'gpt-5', 'display_name': 'GPT-5'}), 'GPT-5');
    });

    test('tokens must be positive integers', () {
      expect(parseTokens('4096'), 4096);
      expect(parseTokens('0'), isNull);
      expect(parseTokens('-1'), isNull);
      expect(parseTokens('x'), isNull);
    });
  });

  group('saving', () {
    test('request keeps unknown fields and the edited revision', () {
      final edited = {...provider(), 'name': 'renamed'};
      final request = saveRequest(edited, secret: 'sk-new');
      expect(request['expected_revision'], 4);
      expect(request['secret'], 'sk-new');
      final saved = request['provider'] as Json;
      for (final field in [
        'preset',
        'binding',
        'oauth',
        'consent',
        'credential',
        'future_field',
      ]) {
        expect(saved[field], provider()[field], reason: field);
      }
      expect(saved['name'], 'renamed');
    });

    test('new providers start at revision zero without models', () {
      final created = newProvider(
        id: 'p2',
        name: 'Local',
        api: 'chat_completions',
        endpoint: 'http://localhost:11434',
      );
      expect(saveRequest(created)['expected_revision'], 0);
      expect(created['models'], isEmpty);
      expect(created['default_model'], '');
      expect(created['authentication'], 'api_key');
    });

    test('fixed endpoints follow their API', () {
      expect(
        api.fixedEndpoint('open_code_go'),
        'https://opencode.ai/zen/go/v1',
      );
      expect(api.fixedEndpoint('responses'), isNull);
    });

    test('catalog provider matches well-known endpoints', () {
      expect(catalogProvider('https://api.openai.com/v1/'), 'openai');
      expect(catalogProvider('https://api.example.com'), isNull);
    });
  });

  group('models', () {
    test('default follows removal and the first addition', () {
      final empty = provider();
      final added = withModels(empty, [model('a'), model('b')]);
      expect(added['default_model'], 'a');
      final picked = {...added, 'default_model': 'b'};
      expect(removeModel(picked, 'b')['default_model'], 'a');
      expect(removeModel(removeModel(picked, 'a'), 'b')['default_model'], '');
    });

    test('renaming moves the default and drops reported metadata', () {
      final current = provider(
        models: [
          {...model('a'), 'display_name': 'A', 'capabilities': {}},
        ],
        fallback: 'a',
      );
      final renamed = renameModel(findModel(current, 'a')!, 'b');
      expect(renamed.containsKey('display_name'), isFalse);
      expect(renamed.containsKey('capabilities'), isFalse);
      final next = replaceModel(current, 'a', renamed);
      expect(next['default_model'], 'b');
      expect(models(next).single['id'], 'b');
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
    test('missing metadata uses protocol defaults', () {
      final configured = configure({'id': 'x'});
      expect(configured['context'], 200000);
      expect(configured['output'], 16384);
      expect(configured['vision'], isTrue);
      expect(configured['tools'], isTrue);
      expect(configured['reasoning'], isFalse);
      expect(configured['default_effort'], 'default');
      expect(configured.containsKey('capabilities'), isFalse);
    });

    test('reported metadata wins over the catalog', () {
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
      expect(configured['display_name'], 'X');
      expect(configured['context'], 400000);
      expect(configured['output'], 128000);
      expect(configured['vision'], isFalse);
      expect(configured['tools'], isFalse);
      expect(configured['reasoning'], isTrue);
      expect(configured['efforts'], ['low', 'high']);
      expect(configured['default_effort'], 'high');
      expect(configured['generates'], ['image']);
    });

    test('catalog inputs without images disable vision', () {
      expect(
        configure(
          {'id': 'x'},
          {
            'inputs': ['text'],
          },
        )['vision'],
        isFalse,
      );
    });

    test('merging skips models the provider has', () {
      final current = provider(models: [model('a')], fallback: 'a');
      final merged = mergeDiscovered(current, [
        {'id': 'a', 'context': 1},
        {'id': 'b', 'context': 64000, 'output': 8000},
      ], {});
      expect(models(merged).map((model) => model['id']), ['a', 'b']);
      expect(findModel(merged, 'a'), model('a'));
      expect(findModel(merged, 'b')!['context'], 64000);
      expect(merged['default_model'], 'a');
      expect(merged['future_field'], 1);
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

    test('values stay raw', () {
      expect(effortValue('xhigh'), 'xhigh');
      expect(effortValue({'budget': 4096}), '4096');
      expect(effortValue({'budget': -1}), '-1');
    });

    test('adding appends and defaults the first effort', () {
      var edited = addEffort(model('a', reasoning: true), 'high')!;
      expect(edited['efforts'], ['high']);
      expect(edited['default_effort'], 'high');
      expect(edited['custom_efforts'], isTrue);
      edited = addEffort(edited, '4096')!;
      expect(edited['efforts'], [
        'high',
        {'budget': 4096},
      ]);
      expect(edited['default_effort'], 'high');
    });

    test('adding rejects invalid, listed and excess efforts', () {
      final listed = {
        ...model('a'),
        'efforts': [
          'low',
          {'budget': 4096},
        ],
      };
      expect(addEffort(listed, 'nope'), isNull);
      expect(addEffort(listed, 'low'), isNull);
      expect(addEffort(listed, '4096'), isNull);
      final full = {
        ...model('a'),
        'efforts': [
          for (var i = 1; i <= maxEfforts; i++) {'budget': i},
        ],
      };
      expect(addEffort(full, 'low'), isNull);
    });

    test('removing the default falls back to the first effort', () {
      var edited = removeEffort({
        ...model('a'),
        'efforts': ['low', 'medium', 'high'],
        'default_effort': 'medium',
      }, 1);
      expect(edited['efforts'], ['low', 'high']);
      expect(edited['default_effort'], 'low');
      expect(edited['custom_efforts'], isTrue);
      edited = removeEffort(edited, 1);
      expect(edited['default_effort'], 'low');
      edited = removeEffort(edited, 0);
      expect(edited['efforts'], isEmpty);
      expect(edited['default_effort'], 'default');
    });

    test('default choice is explicit', () {
      final edited = setDefaultEffort({
        ...model('a'),
        'efforts': ['low', 'high'],
      }, 'low');
      expect(edited['default_effort'], 'low');
      expect(edited['custom_efforts'], isTrue);
    });

    test('initial effort prefers a listed choice', () {
      expect(initialEffort(['low', 'high'], 'high'), 'high');
      expect(initialEffort(['low', 'high'], 'max'), 'low');
      expect(initialEffort([], 'high'), 'default');
      expect(
        initialEffort(
          [
            {'budget': 2048},
          ],
          {'budget': 2048},
        ),
        {'budget': 2048},
      );
    });
  });
}
