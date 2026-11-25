import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/conversation.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

class _Observation extends Fake implements ConversationUpdates {
  Completer<String>? _pending;
  bool _initial = true;
  int closes = 0;
  int disposals = 0;

  @override
  Future<String> next() async {
    if (_initial) {
      _initial = false;
      return jsonEncode({
        'connected': true,
        'snapshot': {'page': <String, dynamic>{}},
      });
    }
    return (_pending = Completer<String>()).future;
  }

  @override
  Future<void> close() async => closes++;

  @override
  void dispose() {
    disposals++;
    final pending = _pending;
    if (pending != null && !pending.isCompleted) {
      pending.completeError(StateError('observation closed'));
    }
  }
}

class _Connection extends Fake implements Connection {
  final watches = <String>[];
  final observations = <_Observation>[];

  @override
  Future<ConversationUpdates> watchConversation({
    required String session,
    bool referenceRows = false,
    bool recent = false,
  }) async {
    watches.add(session);
    final observation = _Observation();
    observations.add(observation);
    return observation;
  }

  @override
  Future<void> close() async {}

  @override
  void dispose() {}
}

Map<String, dynamic> _preview(String message) => {
  'data': {
    'page': {
      'entries': [
        {
          'parts': [
            {'kind': 'text', 'data': message},
          ],
        },
      ],
    },
  },
};

void main() {
  testWidgets(
    'large task list only loads previews near the viewport on return',
    (tester) async {
      const size = 100;
      final versions = List.filled(size, 1);
      final reads = <int>[];
      final pending = Completer<Map<String, dynamic>>();
      final connection = _Connection();
      final navigator = GlobalKey<NavigatorState>();
      Future<void> settle() async => tester.pumpAndSettle();

      List<Map<String, dynamic>> records() => [
        for (var index = 0; index < size; index++)
          {
            'id': '$index',
            'activity': {
              'title': 'Session $index',
              'run': {
                'turn': 'turn-${versions[index]}',
                'status': 'completed',
                if (index == 1 && versions[index] > 1)
                  'finished_ms': 4102444800000,
              },
            },
          },
      ];

      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        snapshot: {'sessions': records()},
        connection: connection,
        command: (kind, arguments) async {
          expect(kind, 'read_conversation');
          final index = int.parse(arguments!['session'] as String);
          reads.add(index);
          if (index == 0) return pending.future;
          return _preview('Preview $index rev ${versions[index]}');
        },
      );
      final session = AppSession.test(hosts: [host]);
      addTearDown(() async {
        await session.close();
        session.dispose();
      });
      tester.view.devicePixelRatio = 1;
      tester.view.physicalSize = const Size(390, 740);
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      await tester.pumpWidget(
        SessionScope(
          session: session,
          child: MaterialApp(
            navigatorKey: navigator,
            theme: SailryTheme.of(Brightness.light),
            locale: const Locale('en'),
            localizationsDelegates: AppLocalizations.localizationsDelegates,
            supportedLocales: AppLocalizations.supportedLocales,
            home: const TasksPage(),
          ),
        ),
      );
      await settle();

      const target = 'Session 0';
      final rows = find.byType(ConversationTask, skipOffstage: false);
      final initialRows = tester.widgetList<ConversationTask>(rows).toList();
      final initialReads = reads.length;
      final moved = find.byWidgetPredicate(
        (widget) => widget is ConversationTask && widget.session['id'] == '1',
      );
      final movedState = tester.state(moved);
      final initialPosition = tester.getTopLeft(moved).dy;
      const targetPreview = 'Preview 0 rev 1';
      expect(reads.where((id) => id == 0), [0]);
      expect(find.text(targetPreview), findsNothing);
      expect(connection.watches, isEmpty);

      await tester.tap(find.text(target));
      await settle();
      expect(connection.watches, ['0']);
      expect(connection.observations.single.closes, 0);
      expect(connection.observations.single.disposals, 0);
      navigator.currentState!.pop();
      await settle();
      final unchangedReturnReads = reads.length - initialReads;
      final unchangedReturnRows = tester
          .widgetList<ConversationTask>(rows)
          .where((row) => !initialRows.contains(row))
          .length;
      expect(connection.observations.single.closes, 1);
      expect(connection.observations.single.disposals, 1);
      expect(find.text(targetPreview), findsNothing);
      pending.complete(_preview(targetPreview));
      await settle();
      expect(find.text(targetPreview), findsOneWidget);
      expect(reads.where((id) => id == 0), [0]);

      await tester.tap(find.text(target));
      await settle();
      expect(connection.watches, ['0', '0']);
      for (final version in [2, 3, 4]) {
        versions[1] = versions[98] = versions[99] = version;
        host.snapshot = {'sessions': records()};
        host.notifyListeners();
        await tester.pump();
      }
      expect(reads.length, initialReads + unchangedReturnReads);
      navigator.currentState!.pop();
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 100));
      expect(reads.length, initialReads + unchangedReturnReads);
      await settle();
      final changedReturnReads = reads.skip(
        initialReads + unchangedReturnReads,
      );
      final returnedRows = tester.widgetList<ConversationTask>(rows).toList();
      final changedReturnRows = returnedRows
          .where((row) => !initialRows.contains(row))
          .length;
      expect(connection.observations.map((item) => item.closes), [1, 1]);
      expect(connection.observations.map((item) => item.disposals), [1, 1]);
      expect(tester.state(moved), same(movedState));
      expect(tester.getTopLeft(moved).dy, lessThan(initialPosition));
      expect(find.text('Preview 1 rev 4'), findsOneWidget);

      expect(initialReads, lessThan(20));
      expect(initialRows.length, lessThan(20));
      expect(unchangedReturnReads, 0);
      expect(unchangedReturnRows, 0);
      expect(changedReturnReads, [1]);
      expect(changedReturnRows, lessThan(20));
      expect(returnedRows.length, lessThan(20));

      await tester.scrollUntilVisible(
        find.text('Session 99'),
        450,
        scrollable: find.byWidgetPredicate(
          (widget) =>
              widget is Scrollable &&
              widget.axisDirection == AxisDirection.down,
        ),
      );
      await settle();
      expect(find.text('Preview 99 rev 4'), findsOneWidget);
      expect(tester.widgetList<ConversationTask>(rows).length, lessThan(20));
      expect(connection.watches, ['0', '0']);
      await tester.pumpWidget(const SizedBox.shrink());
    },
  );
}
