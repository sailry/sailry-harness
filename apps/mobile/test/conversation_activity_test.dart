import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/conversation.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/activity.dart';
import 'package:sailry_mobile/features/conversations/live/progress.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'live_conversations_test.dart' as fixture;

Map<String, dynamic> tool(String id, String name, {String? title}) => {
  'id': id,
  'author': 'assistant',
  'turn': 'turn',
  'parts': [
    {
      'kind': 'tool_call',
      'data': {
        'name': name,
        'arguments': {'title': ?title},
      },
    },
  ],
};

Map<String, dynamic> child(String status) => {
  'run': {'session': 'child', 'turn': 'child-turn', 'status': status},
  'origin': {
    'session': 'session',
    'turn': 'turn',
    'entry': 'spawn',
    'index': 0,
  },
  'name': 'Reviewer',
};

class ChildrenConnection extends fixture.ConnectionFixture {
  final childUpdates = fixture.UpdatesFixture();

  @override
  Future<ConversationUpdates> watchConversation({
    required String session,
  }) async {
    watched.add(session);
    return session == 'child' ? childUpdates : updates;
  }
}

void main() {
  setUp(
    () =>
        TestWidgetsFlutterBinding.ensureInitialized()
            .platformDispatcher
            .accessibilityFeaturesTestValue = const FakeAccessibilityFeatures(
          disableAnimations: true,
        ),
  );
  tearDown(
    () => TestWidgetsFlutterBinding.ensureInitialized().platformDispatcher
        .clearAccessibilityFeaturesTestValue(),
  );
  testWidgets('latest plan and child state', (tester) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    Map<String, dynamic> progress(String description, String state) => {
      'title': 'Review plan',
      'steps': [
        {'description': description, 'state': state},
      ],
    };
    Future<void> render({bool completed = false, bool answer = true}) async {
      final projection = fixture.view(
        status: 'running',
        entries: [
          tool('plan-old', 'update_plan'),
          tool('plan-new', 'update_plan'),
          tool('spawn', 'spawn_agent', title: 'Review security'),
          if (answer) fixture.entry('answer', 'assistant', 'Answer remains'),
        ],
        calls: [
          {
            'turn': 'turn',
            'source': {'entry': 'plan-old', 'index': 0},
            'progress': progress('Stale plan', 'pending'),
          },
          {
            'turn': 'turn',
            'source': {'entry': 'plan-new', 'index': 0},
            'progress': progress(
              'Current plan',
              completed ? 'completed' : 'in_progress',
            ),
          },
          {
            'turn': 'turn',
            'name': 'spawn_agent',
            'state': 'returned',
            'source': {'entry': 'spawn', 'index': 0},
          },
        ],
      );
      projection['snapshot']['page']['children'] = [
        child(completed ? 'completed' : 'running'),
      ];
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.light),
          home: Scaffold(
            body: SingleChildScrollView(
              child: LiveTimeline(
                view: projection,
                session: fixture.session(),
                host: host,
                command: (_, _) async => {},
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
    }

    await render();
    expect(find.text('Stale plan'), findsNothing);
    expect(find.text('Current plan'), findsOneWidget);
    expect(find.text('Review plan  0/1'), findsNothing);
    expect(
      find.descendant(
        of: find.byType(TaskProgress),
        matching: find.text(tr('running')),
      ),
      findsOneWidget,
    );
    expect(find.text('Review security'), findsOneWidget);
    await render(completed: true);
    expect(find.text('Review plan  1/1'), findsNothing);
    expect(
      find.descendant(
        of: find.byType(TaskProgress),
        matching: find.text(tr('completed')),
      ),
      findsOneWidget,
    );
    expect(
      find.descendant(
        of: find.byKey(const ValueKey('child-child')),
        matching: find.text(tr('completed')),
      ),
      findsOneWidget,
    );
    expect(find.text('Answer remains'), findsOneWidget);
    await render(completed: true, answer: false);
    expect(find.text(tr('thinkingNow')), findsNothing);
    expect(
      tester
          .widgetList<ActivityText>(find.byType(ActivityText))
          .singleWhere((label) => label.text == 'Review security')
          .active,
      isTrue,
    );
    await render(completed: true);
    expect(
      tester
          .widgetList<ActivityText>(find.byType(ActivityText))
          .singleWhere((label) => label.text == 'Review security')
          .active,
      isFalse,
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    host.dispose();
  });

  testWidgets('child subscription preserves the parent draft', (tester) async {
    final connection = ChildrenConnection();
    final parent = fixture.session();
    final delegated = {
      ...fixture.session(id: 'child'),
      'delegation': child('running')['origin'],
    };
    final commands = <(String, Map<String, dynamic>?)>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [parent, delegated],
      },
      command: (kind, data) async {
        commands.add((kind, data));
        return {};
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: parent,
      ),
      app,
    );
    final projection = fixture.view(
      status: 'running',
      entries: [tool('spawn', 'spawn_agent', title: 'Review security')],
      calls: [
        {
          'turn': 'turn',
          'name': 'spawn_agent',
          'state': 'returned',
          'source': {'entry': 'spawn', 'index': 0},
        },
      ],
    );
    projection['snapshot']['page']['children'] = [child('running')];
    connection.updates.emit(projection);
    await tester.pump();
    await tester.pump();
    await tester.enterText(find.byType(TextField), 'Keep parent draft');
    expect(connection.watched, ['session']);
    await tester.tap(find.byKey(const ValueKey('child-child')));
    await tester.pump();
    final detail = fixture.view(
      status: 'running',
      entries: [
        {
          ...fixture.entry('child-answer', 'assistant', 'Child output'),
          'turn': 'child-turn',
        },
      ],
    );
    detail['snapshot']['page']['runs'] = [
      {'turn': 'child-turn', 'status': 'running'},
    ];
    connection.childUpdates.emit(detail);
    await tester.pumpAndSettle();
    expect(connection.watched, ['session', 'child']);
    expect(find.text('Child output'), findsOneWidget);
    expect(find.text(tr('conversationChildReadonly')), findsOneWidget);
    expect(find.byType(TextField), findsNothing);
    await tester.tap(find.widgetWithText(FilledButton, tr('stop')));
    await tester.pump();
    expect(commands, hasLength(1));
    expect(commands.single.$1, 'stop_turn');
    expect(commands.single.$2, {'turn': 'child-turn'});
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    expect(find.text('Keep parent draft'), findsOneWidget);
    expect(connection.childUpdates.closed, isTrue);
    expect(connection.updates.closed, isFalse);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });
}
