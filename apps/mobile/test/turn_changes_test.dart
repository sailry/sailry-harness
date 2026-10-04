import 'dart:async';
import 'dart:convert';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/turn_changes.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'live_conversations_test.dart' as fixture;

Map<String, dynamic> checkpoint(String id, String path) => {
  'id': id,
  'path': path,
  'worktree': 'captured-tree',
  'outcome': {
    'kind': 'completed',
    'data': {
      'Ok': {'kind': 'file_written'},
    },
  },
};
Map<String, dynamic> restored(String id) => {
  'kind': 'file_restored',
  'data': {'session': 'session', 'checkpoint': id},
};
Map<String, dynamic> diff() => {
  'kind': 'turn_diff',
  'data': {
    'session': 'session',
    'turn': 'turn',
    'files': [
      {
        'path': 'source.rs',
        'text': '@@ -1 +1 @@\n-old\n+new',
        'additions': 1,
        'deletions': 1,
      },
    ],
  },
};

class RecoveryConnection extends fixture.ConnectionFixture {
  @override
  Future<String> execute({required String request}) async {
    reconciled.add(request);
    return jsonEncode({'Ok': restored('new')});
  }
}

void main() {
  testWidgets('changes belong only to the latest reply and hide during send', (
    tester,
  ) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => diff(),
    );
    final app = AppSession.test(hosts: [host]);
    addTearDown(app.dispose);
    final projection = fixture.view(
      entries: [fixture.entry('answer', 'assistant', 'Done')],
      calls: [
        {'turn': 'turn', 'name': 'package_file_action', 'state': 'returned'},
      ],
    );
    final state = ValueNotifier((projection, false));
    addTearDown(state.dispose);
    await fixture.mount(
      tester,
      Scaffold(
        body: SingleChildScrollView(
          child: ValueListenableBuilder(
            valueListenable: state,
            builder: (context, value, _) => LiveTimeline(
              view: value.$1,
              sending: value.$2,
              session: fixture.session(),
              host: host,
              command: (_, _) async => {},
            ),
          ),
        ),
      ),
      app,
    );
    expect(find.byType(TurnChanges), findsOneWidget);
    state.value = (projection, true);
    await tester.pump();
    expect(find.byType(TurnChanges), findsNothing);
    state.value = (projection, false);
    await tester.pump();
    expect(find.byType(TurnChanges), findsOneWidget);
    final snapshot = Map<String, dynamic>.from(projection['snapshot'] as Map);
    final page = Map<String, dynamic>.from(snapshot['page'] as Map);
    page['runs'] = [
      {'turn': 'turn', 'status': 'completed'},
      {'turn': 'next', 'status': 'queued'},
    ];
    state.value = (
      {
        ...projection,
        'snapshot': {...snapshot, 'page': page},
      },
      false,
    );
    await tester.pump();
    expect(find.byType(TurnChanges), findsNothing);
    page['runs'] = [
      {'turn': 'turn', 'status': 'completed'},
      {'turn': 'next', 'status': 'completed'},
    ];
    state.value = (
      {
        ...projection,
        'snapshot': {...snapshot, 'page': page},
      },
      false,
    );
    await tester.pump();
    expect(find.byType(TurnChanges), findsNothing);
    expect(tester.takeException(), isNull);
  });

  test('inspection is lazy, deduplicated and cached', () async {
    final gate = Completer<Map<String, dynamic>>();
    final commands = <String>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (kind, data) {
        commands.add(kind);
        expect(data, {'session': 'session', 'turn': 'turn'});
        return gate.future;
      },
    );
    final review = ChangeReview(host, 'session', 'turn');
    expect(commands, isEmpty);
    final loading = review.load();
    await review.load();
    expect(commands, ['read_turn_diff']);
    gate.complete(diff());
    await loading;
    await review.load();
    expect(commands, ['read_turn_diff']);
    expect(review.diff!['files'], hasLength(1));
    review.dispose();
  });

  test('recovery pages newest first and stops on conflict', () async {
    final writes = <String>[];
    final cursors = <Object?>[];
    var conflict = true;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (kind, data) async {
        if (kind == 'list_file_checkpoints') {
          cursors.add(data!['before']);
          return {
            'data': {
              'session': 'session',
              'turn': 'turn',
              'files': data['before'] == null
                  ? [checkpoint('new', 'a'), checkpoint('other', 'b')]
                  : [checkpoint('old', 'a')],
              'next': data['before'] == null ? 'cursor' : null,
            },
          };
        }
        expect(kind, 'restore_file_checkpoint');
        expect(data!['worktree'], 'captured-tree');
        final id = data['checkpoint'] as String;
        writes.add(id);
        if (id == 'old' && conflict) {
          throw const CommandFailure('revision_conflict');
        }
        return restored(id);
      },
    );
    final review = ChangeReview(host, 'session', 'turn');
    await review.restore('a');
    expect(writes, ['new', 'old']);
    expect(cursors, [null, 'cursor']);
    expect(review.done, {'new'});
    expect(review.error, isNotNull);
    expect(review.restored('a'), isFalse);
    conflict = false;
    await review.restore('a');
    expect(writes, ['new', 'old', 'old']);
    expect(review.restored('a'), isTrue);
    await review.restore(null);
    expect(writes, ['new', 'old', 'old', 'other']);
    expect(review.restored(null), isTrue);
    review.dispose();
  });

  test(
    'unknown recovery retains target and reconciles the same request',
    () async {
      final connection = RecoveryConnection();
      final writes = <String>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        connection: connection,
        command: (kind, data) async {
          if (kind == 'list_file_checkpoints') {
            return {
              'data': {
                'session': 'session',
                'turn': 'turn',
                'files': [
                  checkpoint('new', 'a'),
                  checkpoint('old', 'a'),
                  checkpoint('other', 'b'),
                ],
              },
            };
          }
          final id = data!['checkpoint'] as String;
          writes.add(id);
          if (id == 'new') {
            throw const CommandFailure(
              'outcome_unknown',
              request: 'original-restore-request',
            );
          }
          return restored(id);
        },
      );
      final review = ChangeReview(host, 'session', 'turn');
      await review.restore('a');
      expect(review.pending, 'original-restore-request');
      expect(writes, ['new']);
      await review.restore(null);
      expect(connection.reconciled, ['original-restore-request']);
      expect(writes, ['new', 'old']);
      expect(review.done, {'new', 'old'});
      expect(review.restored('b'), isFalse);
      review.dispose();
    },
  );

  test('unconfirmed writes are never restored', () async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (kind, data) async {
        expect(kind, 'list_file_checkpoints');
        return {
          'data': {
            'session': 'session',
            'turn': 'turn',
            'files': [
              {
                ...checkpoint('pending', 'a'),
                'outcome': {'kind': 'pending'},
              },
              {
                ...checkpoint('failed', 'a'),
                'outcome': {
                  'kind': 'completed',
                  'data': {
                    'Err': {'code': 'revision_conflict'},
                  },
                },
              },
            ],
          },
        };
      },
    );
    final review = ChangeReview(host, 'session', 'turn');
    await review.restore(null);
    expect(review.done, isEmpty);
    expect(review.error, isNull);
    review.dispose();
  });

  testWidgets(
    'read-only inspection caches across navigation and blocks restore',
    (tester) async {
      final commands = <String>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        command: (kind, _) async {
          commands.add(kind);
          return diff();
        },
      );
      final app = AppSession.test(hosts: [host]);
      await fixture.mount(
        tester,
        Scaffold(
          body: TurnChanges(
            host: host,
            session: 'session',
            turn: 'turn',
            worktree: 'tree',
          ),
        ),
        app,
      );
      expect(commands, isEmpty);
      await tester.tap(find.text(tr('turnChanges')));
      await tester.pumpAndSettle();
      expect(commands, ['read_turn_diff']);
      expect(
        tester
            .widget<TextButton>(
              find.widgetWithText(TextButton, tr('turnUndoAll')),
            )
            .onPressed,
        isNull,
      );
      expect(
        tester
            .widget<TextButton>(find.widgetWithText(TextButton, tr('turnUndo')))
            .onPressed,
        isNull,
      );
      await tester.tap(find.text(tr('turnChanges')));
      await tester.pumpAndSettle();
      await tester.tap(find.textContaining('+1'));
      await tester.pumpAndSettle();
      expect(commands, ['read_turn_diff']);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      app.dispose();
    },
  );
}
