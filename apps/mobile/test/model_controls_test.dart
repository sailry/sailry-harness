import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/configuration.dart';
import 'package:sailry_mobile/features/conversations/live/model_controls.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'live_conversations_test.dart' as fixture;

class ConfigurationConnection extends fixture.ConnectionFixture {
  @override
  Future<String> execute({required String request}) async {
    reconciled.add(request);
    if (reconciled.length == 1) {
      return jsonEncode({
        'Err': {'code': 'unavailable', 'message': 'Connection interrupted'},
      });
    }
    return jsonEncode({
      'Ok': {
        'kind': 'session',
        'data': {
          'revision': 10,
          'config': {'mode': 'plan', 'permission': 'ask'},
        },
      },
    });
  }
}

void main() {
  testWidgets('model and effort save the captured revision', (tester) async {
    tester.view.physicalSize = const Size(390, 844);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final providers = [
      for (final id in ['a', 'b'])
        {
          'id': id,
          'enabled': true,
          'name': 'Provider $id',
          'credential': {'id': 'key-$id'},
          'models': [
            {
              'id': 'model-$id',
              'reasoning': true,
              'default_effort': 'medium',
              'efforts': ['low', 'medium', 'high'],
            },
          ],
        },
    ];
    final saved = <Map<String, dynamic>>[];
    var revision = 9;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {'providers': providers},
      command: (kind, data) async {
        expect(kind, 'set_session_config');
        expect(data!['expected_revision'], revision);
        saved.add(data);
        return {
          'kind': 'session',
          'data': {'revision': ++revision, 'config': data['config']},
        };
      },
    );
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Scaffold(
          body: ConversationConfiguration(
            host: host,
            session: {
              'id': 'session',
              'revision': 9,
              'config': {
                'provider': 'a',
                'model': 'model-a',
                'effort': 'medium',
                'mode': 'code',
                'permission': 'ask',
              },
            },
          ),
        ),
      ),
    );
    final label = find.text('medium');
    final modelButton = find.byKey(const ValueKey('choose-model'));
    expect(
      tester.getCenter(modelButton).dy - tester.getCenter(label).dy,
      lessThanOrEqualTo(40),
    );
    await tester.tap(find.byKey(const ValueKey('choose-model')));
    await tester.pumpAndSettle();
    expect(find.byType(DropdownButtonFormField<String>), findsNothing);
    expect(find.widgetWithText(ListTile, 'model-a'), findsOneWidget);
    expect(find.text('model-b'), findsNothing);
    await tester.tap(find.text('Provider a'));
    await tester.pumpAndSettle();
    expect(find.widgetWithText(ListTile, 'model-a'), findsNothing);
    expect(saved, isEmpty);
    await tester.tap(find.text('Provider b'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('model-b'));
    await tester.pumpAndSettle();
    expect(saved, hasLength(1));
    final slider = find.byKey(const ValueKey('reasoning-slider'));
    await tester.tapAt(tester.getTopRight(slider) + const Offset(-24, 24));
    await tester.pumpAndSettle();
    expect(saved, hasLength(2));
    expect(find.text('high'), findsOneWidget);
    expect(tester.widget<Slider>(slider).semanticFormatterCallback!(2), 'high');
    expect(
      tester.widget<ModelControls>(find.byType(ModelControls)).config['effort'],
      'high',
    );
    await tester.tap(find.byTooltip(tr('resetReasoning')));
    await tester.pumpAndSettle();
    expect(
      tester.widget<ModelControls>(find.byType(ModelControls)).config['effort'],
      'medium',
    );
    expect(saved, hasLength(3));
    expect(find.text(tr('save')), findsNothing);
    final mode = find.byKey(const ValueKey('configuration-mode'));
    final permission = find.byKey(const ValueKey('configuration-permission'));
    expect(tester.getSize(mode).width, tester.getSize(permission).width);
    expect(tester.getSize(mode).width, tester.view.physicalSize.width);
    await tester.tap(find.text(tr('conversationPlan')));
    await tester.pumpAndSettle();
    await tester.tap(find.text(tr('conversationFull')));
    await tester.pumpAndSettle();
    expect(saved, hasLength(5));
    expect(saved.last, {
      'session': 'session',
      'expected_revision': 13,
      'config': {
        'provider': 'b',
        'model': 'model-b',
        'credential': {'id': 'key-b'},
        'effort': 'medium',
        'mode': 'plan',
        'permission': 'full',
      },
    });
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await host.close();
    host.dispose();
  });

  testWidgets('serialized autosave preserves failed drafts', (tester) async {
    final pending = Completer<Map<String, dynamic>>();
    var calls = 0;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {},
      command: (kind, data) {
        calls++;
        expect(data!['expected_revision'], 9);
        return pending.future;
      },
    );
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Scaffold(
          body: ConversationConfiguration(
            host: host,
            session: {
              'id': 'session',
              'revision': 9,
              'config': {'mode': 'code', 'permission': 'ask'},
            },
          ),
        ),
      ),
    );
    final configFinder = find.byType(ConversationConfiguration);
    final before = tester.getRect(configFinder);
    await tester.tap(find.text(tr('conversationPlan')));
    await tester.pump();
    final pendingMode = tester.widget<SegmentedButton<String>>(
      find.byKey(const ValueKey('configuration-mode')),
    );
    expect(pendingMode.onSelectionChanged, isNotNull);
    expect(pendingMode.selected, {'plan'});
    expect(tester.getRect(configFinder), before);
    await tester.tap(find.text(tr('conversationFull')), warnIfMissed: false);
    await tester.pump();
    expect(calls, 1);
    pending.completeError(CommandFailure('revision_conflict'));
    await tester.pumpAndSettle();
    final mode = tester.widget<SegmentedButton<String>>(
      find.byKey(const ValueKey('configuration-mode')),
    );
    expect(mode.selected, {'plan'});
    expect(mode.onSelectionChanged, isNotNull);
    expect(find.text(tr('conversationConflict')), findsOneWidget);
    expect(
      find.descendant(
        of: find.byKey(const ValueKey('app-toast')),
        matching: find.text(tr('conversationConflict')),
      ),
      findsOneWidget,
    );
    expect(find.byType(ConversationConfiguration), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox());
    await host.close();
    host.dispose();
  });

  testWidgets('unknown configuration retains its request and revision', (
    tester,
  ) async {
    final connection = ConfigurationConnection();
    var writes = 0;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (kind, data) async {
        expect(kind, 'set_session_config');
        writes++;
        if (writes == 1) {
          expect(data?['expected_revision'], 9);
          throw const CommandFailure(
            'outcome_unknown',
            request: 'configuration-original',
          );
        }
        expect(data?['expected_revision'], 10);
        return {
          'kind': 'session',
          'data': {'revision': 11, 'config': data?['config']},
        };
      },
    );
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: Scaffold(
          body: ConversationConfiguration(
            host: host,
            session: {
              'id': 'session',
              'revision': 9,
              'config': {'mode': 'code', 'permission': 'ask'},
            },
          ),
        ),
      ),
    );
    await tester.tap(find.text(tr('conversationPlan')));
    await tester.pumpAndSettle();
    final mode = find.byKey(const ValueKey('configuration-mode'));
    expect(tester.widget<SegmentedButton<String>>(mode).selected, {'plan'});
    expect(
      tester.widget<SegmentedButton<String>>(mode).onSelectionChanged,
      isNull,
    );
    await tester.tap(find.text(tr('conversationCheckResult')));
    await tester.pumpAndSettle();
    expect(connection.reconciled, ['configuration-original']);
    expect(find.text(tr('conversationCheckResult')), findsOneWidget);
    await tester.tap(find.text(tr('conversationCheckResult')));
    await tester.pumpAndSettle();
    expect(connection.reconciled, [
      'configuration-original',
      'configuration-original',
    ]);
    expect(find.text(tr('conversationCheckResult')), findsNothing);
    expect(
      tester.widget<SegmentedButton<String>>(mode).onSelectionChanged,
      isNotNull,
    );
    await tester.tap(find.text(tr('conversationFull')));
    await tester.pumpAndSettle();
    expect(writes, 2);
    expect(tester.takeException(), isNull);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox());
    await host.close();
    host.dispose();
  });
}
