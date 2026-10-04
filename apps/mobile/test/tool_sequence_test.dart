import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/live/presentation.dart';
import 'package:sailry_mobile/features/conversations/live/tool_sequence.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'live_conversations_test.dart' as fixture;
import 'package:sailry_mobile/features/conversations/live/activity.dart';
import 'package:sailry_mobile/features/conversations/live/progress.dart';

class Transcript {
  final entries = <Map<String, dynamic>>[];
  final calls = <Map<String, dynamic>>[];

  void tool(
    String id, {
    String? name,
    String state = 'returned',
    bool todo = false,
    bool standalone = false,
  }) {
    entries.add({
      'id': id,
      'turn': 'turn',
      'author': 'assistant',
      'parts': [
        {
          'kind': 'tool_call',
          'data': {'name': name ?? id, 'arguments': <String, dynamic>{}},
        },
        {
          'kind': 'tool_result',
          'data': {'result': 'Output $id', 'images': <Object>[]},
        },
      ],
    });
    calls.add({
      'name': name ?? id,
      'turn': 'turn',
      'source': {'entry': id, 'index': 0},
      'response': {'entry': id, 'index': 1},
      'state': state,
      if (standalone || todo) 'grouping': 'standalone',
      if (todo)
        'progress': {
          'title': 'Plan',
          'steps': [
            {'description': 'Verify behavior', 'state': 'completed'},
          ],
        },
    });
  }

  void text(String id, String text) =>
      entries.add(fixture.entry(id, 'assistant', text));

  Map<String, dynamic> view({String status = 'completed'}) => fixture.view(
    entries: List.of(entries),
    calls: List.of(calls),
    status: status,
  );
}

Future<void> mount(
  WidgetTester tester,
  ValueNotifier<Map<String, dynamic>> projection, {
  ConversationCommand? command,
}) async {
  final host = HostConnection.test(
    id: 'node',
    label: 'Node',
    command: (_, _) async => {},
  );
  final app = AppSession.test(hosts: [host]);
  addTearDown(app.dispose);
  await fixture.mount(
    tester,
    Scaffold(
      body: SingleChildScrollView(
        child: ValueListenableBuilder(
          valueListenable: projection,
          builder: (context, value, _) => LiveTimeline(
            view: value,
            session: fixture.session(),
            host: host,
            command: command ?? (_, _) async => {},
          ),
        ),
      ),
    ),
    app,
  );
  await tester.pumpAndSettle();
}

Future<void> tap(WidgetTester tester, Finder finder) async {
  await tester.ensureVisible(finder);
  await tester.tap(finder);
  await tester.pumpAndSettle();
}

void main() {
  for (final todo in [false, true]) {
    testWidgets(
      'returned ${todo ? 'plan' : 'tool'} owns activity until new content',
      (tester) async {
        final transcript = Transcript()..tool('inspect', todo: todo);
        final projection = ValueNotifier(transcript.view(status: 'running'));
        addTearDown(projection.dispose);
        await mount(tester, projection);
        expect(find.text(tr('thinkingNow')), findsNothing);
        expect(find.byKey(const ValueKey('activity-loading')), findsNothing);
        expect(
          find.byWidgetPredicate(
            (widget) => widget is ActivityText && widget.active,
          ),
          findsOneWidget,
        );
        if (todo) {
          final card = tester.widget<Card>(
            find.descendant(
              of: find.byType(TaskProgress),
              matching: find.byType(Card),
            ),
          );
          expect(card.clipBehavior, Clip.antiAlias);
          expect(card.color!.a, lessThan(1));
          expect(find.text('Plan  1/1'), findsNothing);
        }
        transcript.entries.add({
          'id': 'reasoning',
          'turn': 'turn',
          'author': 'assistant',
          'parts': [
            {'kind': 'thinking', 'data': 'Checking the result'},
          ],
        });
        projection.value = transcript.view(status: 'running');
        await tester.pumpAndSettle();
        expect(find.text(tr('thinkingNow')), findsOneWidget);
        final active = tester
            .widgetList<ActivityLabel>(find.byType(ActivityLabel))
            .where((label) => label.running);
        expect(active.single.label, tr('thinkingNow'));
        projection.value = transcript.view(status: 'cancelled');
        await tester.pumpAndSettle();
        expect(find.byKey(const ValueKey('activity-loading')), findsNothing);
        await tester.pumpWidget(const SizedBox());
      },
    );
  }

  testWidgets(
    'nonzero exits stay neutral and only actual errors fail the group',
    (tester) async {
      final transcript = Transcript()
        ..tool('first', name: 'run_command')
        ..tool('last', name: 'run_command');
      Map<String, dynamic> result(Map<String, dynamic> outcome) => {
        'kind': 'command_result',
        'data': {
          'outcome': outcome,
          'stdout': {'text': ''},
          'stderr': {'text': 'diagnostic output'},
        },
      };
      void replace(int index, Map<String, dynamic> value) {
        (transcript.entries[index]['parts'] as List)[1]['data']['result'] =
            value;
      }

      replace(0, result({'kind': 'exited', 'data': 7}));
      replace(1, result({'kind': 'exited', 'data': 1}));
      final projection = ValueNotifier(transcript.view());
      addTearDown(projection.dispose);
      await mount(tester, projection);
      await tap(tester, find.text(tr('completed')).first);
      final exit = tr('toolExitCode').replaceAll('{code}', '1');
      expect(find.text(exit), findsOneWidget);
      expect(find.text(tr('toolFailed')), findsNothing);
      for (final failure in [
        result({'kind': 'timed_out'}),
        result({'kind': 'signal', 'data': 9}),
        {
          'error': {'code': 'unavailable', 'message': 'command launch failed'},
        },
      ]) {
        replace(0, failure);
        projection.value = transcript.view(status: 'running');
        await tester.pumpAndSettle();
        expect(find.text(tr('toolFailed')), findsNothing);
        final heading = tester.widget<ActivityLabel>(
          find.byType(ActivityLabel).first,
        );
        expect(
          heading.color,
          Theme.of(
            tester.element(find.byType(ActivityLabel).first),
          ).colorScheme.error,
        );
      }
      for (final outcome in [
        {'kind': 'cancelled'},
        {'kind': 'unknown', 'data': 'wait failed'},
        {'kind': 'exited', 'data': 2},
      ]) {
        replace(0, result(outcome));
        projection.value = transcript.view();
        await tester.pumpAndSettle();
        expect(find.text(tr('toolFailed')), findsNothing);
        expect(find.text(exit), findsOneWidget);
      }
    },
  );

  testWidgets(
    'standalone rows split groups while the final answer stays visible',
    (tester) async {
      final transcript = Transcript()
        ..tool('inspect')
        ..text('blank', ' \n\t')
        ..tool('search')
        ..text('update', 'Progress update')
        ..tool('before-plan')
        ..tool('plan', todo: true)
        ..tool('verify-one', name: 'verify')
        ..tool('verify-two', name: 'verify')
        ..tool('external', standalone: true)
        ..tool('check')
        ..tool('finish')
        ..text('answer', 'Final answer');
      final projection = ValueNotifier(transcript.view());
      addTearDown(projection.dispose);
      await mount(tester, projection);
      expect(find.text('Final answer'), findsOneWidget);
      expect(find.byType(ToolSequence), findsNothing);
      await tap(tester, find.text(tr('completed')).first);
      expect(find.text('Verify behavior'), findsOneWidget);
      expect(find.text('external'), findsOneWidget);
      expect(find.text('Progress update'), findsOneWidget);
      expect(find.text('before-plan'), findsOneWidget);
      expect(find.text('inspect'), findsNothing);
      expect(find.text('search'), findsOneWidget);
      final groups = tester.widgetList<ToolSequence>(find.byType(ToolSequence));
      expect(groups.map((group) => group.tools.length), [2, 2, 2]);
      expect(
        groups
            .expand((group) => group.tools)
            .any((tool) => tool.call['grouping'] == 'standalone'),
        isFalse,
      );
      await tap(tester, find.text('search'));
      expect(find.text('inspect'), findsOneWidget);
      await tap(tester, find.text('inspect'));
      expect(find.text('Output inspect'), findsOneWidget);
      await tap(tester, find.text(tr('completed')).first);
      expect(find.text('Final answer'), findsOneWidget);
      expect(find.text('Output inspect'), findsNothing);
      await tap(tester, find.text(tr('completed')).first);
      expect(find.text('Output inspect'), findsOneWidget);
      await tap(tester, find.text('search').first);
      expect(find.text('Output inspect'), findsNothing);
      expect(find.text('Verify behavior'), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets(
    'latest activity stays folded and manual choices survive streaming',
    (tester) async {
      final transcript = Transcript()
        ..tool('one', name: 'inspect')
        ..tool('two', name: 'search', state: 'running');
      final projection = ValueNotifier(transcript.view(status: 'running'));
      addTearDown(projection.dispose);
      await mount(tester, projection);
      expect(find.text('search'), findsOneWidget);
      expect(find.text('inspect'), findsNothing);
      expect(find.byKey(const ValueKey('activity-loading')), findsNothing);
      expect(
        find.byWidgetPredicate(
          (widget) => widget is ActivityText && widget.active,
        ),
        findsOneWidget,
      );
      expect(find.text(tr('thinkingNow')), findsNothing);
      expect(find.text(tr('conversationToolRunning')), findsNothing);
      await tap(tester, find.text('search'));
      await tap(tester, find.text('inspect'));
      expect(find.text('Output one'), findsOneWidget);
      transcript.calls[1]['state'] = 'returned';
      transcript.tool('three', name: 'verify', state: 'running');
      projection.value = transcript.view(status: 'running');
      await tester.pumpAndSettle();
      expect(find.text('verify'), findsNWidgets(2));
      expect(find.text('Output one'), findsOneWidget);
      await tap(tester, find.text('verify').first);
      expect(find.text('inspect'), findsNothing);
      transcript.calls.last['state'] = 'returned';
      projection.value = transcript.view(status: 'running');
      await tester.pumpAndSettle();
      expect(find.text('verify'), findsOneWidget);
      expect(find.text('inspect'), findsNothing);
      expect(find.text(tr('thinkingNow')), findsNothing);
      expect(
        tester
            .widgetList<ActivityLabel>(find.byType(ActivityLabel))
            .where((label) => label.running)
            .single
            .label,
        'verify',
      );
      transcript.text('answer', 'Final answer');
      projection.value = transcript.view();
      await tester.pumpAndSettle();
      expect(find.text('verify'), findsNothing);
      expect(find.text('Final answer'), findsOneWidget);
      await tap(tester, find.text(tr('completed')));
      expect(find.text('verify'), findsOneWidget);
      expect(find.text('inspect'), findsNothing);
      await tap(tester, find.text('verify'));
      expect(find.text('Output one'), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets(
    'approval waits for manual expansion and retains command routing',
    (tester) async {
      final transcript = Transcript()
        ..tool('inspect')
        ..tool('change', state: 'waiting');
      transcript.calls.last['approval'] = {
        'id': 'approval',
        'state': 'pending',
      };
      final projection = ValueNotifier(transcript.view(status: 'running'));
      addTearDown(projection.dispose);
      final commands = <(String, Map<String, dynamic>?)>[];
      await mount(
        tester,
        projection,
        command: (kind, data) async {
          commands.add((kind, data));
          return {};
        },
      );
      expect(find.text('change'), findsOneWidget);
      expect(find.text(tr('allowShort')), findsNothing);
      await tap(tester, find.text('change'));
      await tap(tester, find.text(tr('allowShort')));
      expect(commands.single.$1, 'resolve_approval');
      expect(commands.single.$2, {
        'session': 'session',
        'approval': 'approval',
        'decision': 'approve',
      });
      await tester.pumpWidget(const SizedBox());
    },
  );

  testWidgets(
    'activity animates until completion and respects reduced motion',
    (tester) async {
      Future<void> render(bool active, {bool reduced = false}) =>
          tester.pumpWidget(
            MaterialApp(
              home: MediaQuery(
                data: MediaQueryData(disableAnimations: reduced),
                child: Scaffold(
                  body: ActivityLabel(label: 'Inspect', running: active),
                ),
              ),
            ),
          );
      await render(true);
      await tester.pump(const Duration(milliseconds: 300));
      expect(find.byType(ShaderMask), findsOneWidget);
      expect(find.byKey(const ValueKey('activity-loading')), findsNothing);
      expect(
        find.byWidgetPredicate(
          (widget) => widget is ActivityText && widget.active,
        ),
        findsOneWidget,
      );
      expect(tester.binding.hasScheduledFrame, isTrue);
      await render(true, reduced: true);
      await tester.pumpAndSettle();
      expect(find.byType(ShaderMask), findsNothing);
      expect(find.text('Inspect'), findsOneWidget);
      await render(false);
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('activity-loading')), findsNothing);
      await tester.pumpWidget(const SizedBox());
    },
  );
}
