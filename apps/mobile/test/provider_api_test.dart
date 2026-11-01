import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/providers/api.dart' as api;

void main() {
  test('opaque provider APIs retain their identity', () {
    expect(api.choices('plugin').where((value) => value == 'plugin'), [
      'plugin',
    ]);
    expect(api.editable('plugin'), isFalse);
    expect(api.choices('future_transport').last, 'future_transport');
    expect(api.editable('future_transport'), isFalse);
    expect(api.choices('chat_completions'), api.values);
    expect(api.editable('chat_completions'), isTrue);
    expect(api.values, isNot(contains('plugin')));
  });

  group('endpoint policy', () {
    final catalog =
        jsonDecode(
              File(
                '../../assets/catalogs/model-presets.json',
              ).readAsStringSync(),
            )
            as Map<String, dynamic>;
    final entries = [
      for (final preset in catalog['builtin'] as List)
        <String, dynamic>{
          'source': {'kind': 'builtin'},
          'preset': preset,
        },
    ];

    Map<String, dynamic> provider(String id, String api, {Object? route}) => {
      'api': api,
      'preset': {
        'id': id,
        'source': {'kind': 'builtin'},
        'route': route,
      },
      'endpoint': 'https://user.example/v1',
    };

    test('compatible and Azure presets keep custom endpoints', () {
      for (final (id, protocol) in [
        ('compatible-responses', 'responses'),
        ('compatible-responses', 'chat_completions'),
        ('compatible-chat', 'chat_completions'),
        ('compatible-anthropic', 'anthropic'),
        ('compatible-gemini', 'gemini'),
        ('azure-openai', 'azure_open_ai'),
        ('azure-ai', 'azure_ai'),
      ]) {
        expect(
          api.editableEndpoint(provider(id, protocol), entries),
          isTrue,
          reason: '$id/$protocol',
        );
      }
    });

    test('fixed builtin endpoints stay fixed', () {
      for (final (id, protocol) in [
        ('openai', 'responses'),
        ('anthropic', 'anthropic'),
        ('gemini', 'gemini'),
        ('opencode-go', 'open_code_go'),
      ]) {
        expect(
          api.editableEndpoint(provider(id, protocol), entries),
          isFalse,
          reason: id,
        );
      }
    });

    test('route identity includes protocol region and product', () {
      final entry = {
        'source': {'kind': 'builtin'},
        'preset': {
          'id': 'mixed',
          'routes': [
            {
              'protocol': 'openai-responses',
              'region': 'global',
              'product': 'coding',
              'endpoint': 'https://fixed.example/v1',
            },
            {
              'protocol': 'openai-responses',
              'region': 'china',
              'product': 'metered',
              'endpoint': '',
            },
          ],
        },
      };
      for (final (region, product, expected) in [
        ('global', 'coding', false),
        ('china', 'metered', true),
        ('global', 'metered', null),
      ]) {
        expect(
          api.editableEndpoint(
            provider(
              'mixed',
              'responses',
              route: {
                'protocol': 'openai-responses',
                'region': region,
                'product': product,
              },
            ),
            [entry],
          ),
          expected,
        );
      }
    });

    test('custom sources and manual APIs retain endpoint editing', () {
      for (final source in [
        {'kind': 'partner'},
        {
          'kind': 'package',
          'data': {'name': 'fixture'},
        },
      ]) {
        expect(
          api.editableEndpoint({
            ...provider('openai', 'responses'),
            'preset': {'id': 'openai', 'source': source},
          }, entries),
          isTrue,
        );
      }
      expect(api.editableEndpoint({'api': 'responses'}, []), isTrue);
      expect(api.editableEndpoint({'api': 'open_code_go'}, []), isFalse);
      expect(
        api.editableEndpoint(provider('missing', 'responses'), entries),
        isNull,
      );
    });
  });
}
