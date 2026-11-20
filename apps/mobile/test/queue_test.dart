import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/queue.dart';
import 'package:sailry_mobile/features/conversations/live/turn_draft.dart';
import 'package:sailry_mobile/l10n/strings.dart';

Map<String, dynamic> view(
  List<Map<String, dynamic>> items, {
  Map<String, dynamic> guidance = const {},
  bool paused = false,
}) => {
  'snapshot': {
    'page': {
      'queue': {'items': items, 'paused': paused},
    },
  },
  'guidance': guidance,
};

void main() {
  testWidgets('queued notices remain named and removable, not editable', (
    tester,
  ) async {
    final updates = ValueNotifier({
      ...view([
        {
          'turn': 'callback',
          'kind': 'task',
          'notice': true,
          'revision': 4,
          'preview': 'Callback prompt for the model',
        },
      ]),
      'connected': true,
    });
    addTearDown(updates.dispose);
    final commands = <(String, Map<String, dynamic>)>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: LiveQueue(
            view: updates,
            session: 'parent',
            command: (kind, data) async {
              commands.add((kind, data));
              return {};
            },
          ),
        ),
      ),
    );
    expect(find.text(tr('backgroundNoticeTitle')), findsOneWidget);
    expect(find.text('Callback prompt for the model'), findsNothing);
    expect(find.byTooltip(tr('edit')), findsNothing);
    expect(find.byTooltip(tr('send')), findsNothing);
    expect(summarize(updates.value, null).count, 1);
    await tester.tap(find.byTooltip(tr('delete')));
    await tester.pumpAndSettle();
    expect(commands, hasLength(1));
    expect(commands.single.$1, 'remove_queued_turn');
    expect(commands.single.$2, {'turn': 'callback', 'expected_revision': 4});
  });

  testWidgets('child queue can withdraw guidance and remove retained input', (
    tester,
  ) async {
    final items = <Map<String, dynamic>>[
      {
        'turn': 'guidance',
        'kind': 'task',
        'guidance': 'child-run',
        'revision': 3,
        'preview': 'Pending guidance',
      },
      {
        'turn': 'retained',
        'kind': 'task',
        'revision': 5,
        'preview': 'Retained input',
      },
    ];
    Map<String, dynamic> current() => {
      ...view(List.of(items)),
      'connected': true,
    };
    final updates = ValueNotifier(current());
    addTearDown(updates.dispose);
    final commands = <(String, Map<String, dynamic>)>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: LiveQueue(
            view: updates,
            session: 'child',
            delegated: true,
            command: (_, _) async => fail('Unexpected direct command'),
            mutate: (kind, data) async {
              commands.add((kind, data));
              items.removeWhere((item) => item['turn'] == data['turn']);
              updates.value = current();
              return true;
            },
          ),
        ),
      ),
    );
    for (final key in ['edit', 'send', 'moveUp', 'pauseQueue', 'resumeQueue']) {
      expect(find.text(tr(key)), findsNothing);
      expect(find.byTooltip(tr(key)), findsNothing);
    }
    await tester.tap(find.byTooltip(tr('guidanceWithdraw')));
    await tester.pumpAndSettle();
    expect(commands.map((entry) => [entry.$1, entry.$2]), [
      [
        'withdraw_guidance',
        {'turn': 'guidance', 'expected_revision': 3},
      ],
    ]);
    expect(find.text('Pending guidance'), findsNothing);
    expect(find.text('Retained input'), findsOneWidget);

    await tester.tap(find.byTooltip(tr('delete')));
    await tester.pumpAndSettle();
    expect(commands.map((entry) => [entry.$1, entry.$2]), [
      [
        'withdraw_guidance',
        {'turn': 'guidance', 'expected_revision': 3},
      ],
      [
        'remove_queued_turn',
        {'turn': 'retained', 'expected_revision': 5},
      ],
    ]);
    expect(find.text('Retained input'), findsNothing);
    expect(find.text(tr('queueEmpty')), findsOneWidget);
  });

  group('summarize', () {
    test('is empty without queued input', () {
      expect(summarize({}, null), (count: 0, paused: false, guiding: false));
    });

    test('marks pending guidance among queued input', () {
      final state = summarize(
        view(
          [
            {'turn': 'next'},
            {'turn': 'guide', 'guidance': 'running'},
          ],
          guidance: {'guide': 'pending'},
        ),
        null,
      );
      expect(state, (count: 2, paused: false, guiding: true));
    });

    test('counts guidance until its turn is projected', () {
      const GuidancePreview admitting = (
        turn: null,
        text: 'Focus',
        attachments: [],
      );
      const GuidancePreview admitted = (
        turn: 'guide',
        text: 'Focus',
        attachments: [],
      );
      expect(summarize(view([]), admitting), (
        count: 1,
        paused: false,
        guiding: true,
      ));
      expect(
        summarize(
          view(
            [
              {'turn': 'guide', 'guidance': 'running'},
            ],
            guidance: {'guide': 'pending'},
          ),
          admitted,
        ),
        (count: 1, paused: false, guiding: true),
      );
      expect(summarize(view([], guidance: {'guide': 'delivered'}), admitted), (
        count: 0,
        paused: false,
        guiding: false,
      ));
    });

    test('reports a paused queue', () {
      expect(
        summarize(
          view([
            {'turn': 'next'},
          ], paused: true),
          null,
        ),
        (count: 1, paused: true, guiding: false),
      );
    });
  });

  group('describe', () {
    String tr(String key) => switch (key) {
      'queueCount' => '{count} queued',
      'queuePaused' => 'Paused · {count}',
      'queueGuidance' => '{queue}, guidance waiting',
      _ => key,
    };

    test('states the count', () {
      expect(
        describe((count: 2, paused: false, guiding: false), tr),
        '2 queued',
      );
    });

    test('adds waiting guidance to a paused queue', () {
      expect(
        describe((count: 1, paused: true, guiding: true), tr),
        'Paused · 1, guidance waiting',
      );
    });
  });
}
