import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/providers/controller.dart';
import 'package:sailry_mobile/runtime/session.dart';

Map<String, dynamic> provider() => {
  'id': 'fixture',
  'revision': 4,
  'name': 'Original',
  'api': 'responses',
  'endpoint': 'https://original.example/v1',
  'credential': {'id': 'stored-key'},
  'models': [
    {'id': 'original'},
  ],
};

void main() {
  test('endpoint policy uses the execution Node directory', () async {
    final requests = <(String, Map<String, dynamic>?)>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (kind, data) async {
        requests.add((kind, data));
        return {
          'kind': 'channel_presets',
          'data': {
            'entries': [
              {
                'source': {'kind': 'builtin'},
                'preset': {
                  'id': 'custom-responses',
                  'routes': [
                    {'protocol': 'openai-responses', 'endpoint': ''},
                  ],
                },
              },
            ],
          },
        };
      },
    );
    final controller = ProviderController(host, {
      ...provider(),
      'preset': {
        'id': 'custom-responses',
        'source': {'kind': 'builtin'},
      },
    });
    addTearDown(controller.dispose);
    expect(await controller.editableEndpoint(), isTrue);
    expect(requests, hasLength(1));
    expect(requests.single.$1, 'list_channel_presets');
    expect(requests.single.$2, {'refresh_partners': false});
  });

  group('saving', () {
    for (final failure in [
      'unavailable',
      'revision_conflict',
      'invalid_request',
    ]) {
      test('$failure keeps the saved copy for explicit retry', () async {
        final original = provider();
        final draft = {...original, 'endpoint': 'https://edited.example/v1'};
        final requests = <Map<String, dynamic>>[];
        final host = HostConnection.test(
          id: 'node',
          label: 'Node',
          command: (kind, data) async {
            expect(kind, 'save_provider');
            requests.add(data!);
            if (requests.length == 1) throw CommandFailure(failure);
            return {
              'data': {...draft, 'revision': 5},
            };
          },
        );
        final controller = ProviderController(host, original);
        addTearDown(controller.dispose);
        await expectLater(
          controller.save(draft, secret: 'new-key'),
          throwsA(isA<CommandFailure>()),
        );
        expect(controller.provider, original);
        expect(controller.revision, 4);
        expect(controller.busy, isFalse);
        expect(draft['endpoint'], 'https://edited.example/v1');
        expect(await controller.save(draft, secret: 'new-key'), isTrue);
        expect(controller.provider, {...draft, 'revision': 5});
        expect(requests.map((request) => request['expected_revision']), [4, 4]);
        expect(requests.map((request) => request['secret']), [
          'new-key',
          'new-key',
        ]);
      });
    }

    test('pending saves reject duplicate submission', () async {
      final response = Completer<Map<String, dynamic>>();
      var requests = 0;
      final original = provider();
      final draft = {...original, 'name': 'Edited'};
      final controller = ProviderController(
        HostConnection.test(
          id: 'node',
          label: 'Node',
          command: (_, _) {
            requests++;
            return response.future;
          },
        ),
        original,
      );
      addTearDown(controller.dispose);
      final saving = controller.save(draft);
      expect(controller.busy, isTrue);
      expect(controller.provider, original);
      expect(await controller.save(draft), isFalse);
      expect(requests, 1);
      response.complete({
        'data': {...draft, 'revision': 5},
      });
      expect(await saving, isTrue);
      expect(controller.busy, isFalse);
      expect(controller.provider['name'], 'Edited');
    });

    test('leaving the page drains admitted saves safely', () async {
      final response = Completer<Map<String, dynamic>>();
      final controller = ProviderController(
        HostConnection.test(
          id: 'node',
          label: 'Node',
          command: (_, _) => response.future,
        ),
        provider(),
      );
      final saving = controller.save({...provider(), 'name': 'Edited'});
      controller.dispose();
      response.complete({
        'data': {...provider(), 'name': 'Edited', 'revision': 5},
      });
      expect(await saving, isTrue);
      expect(await controller.save(provider()), isFalse);
    });
  });
}
