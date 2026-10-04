import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/git_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';

import 'live_resources_test.dart' as fixture;

Map<String, dynamic> response(String kind) => {
  'data': switch (kind) {
    'inspect_git' => {
      'kind': 'ready',
      'head': 'head',
      'branch': 'main',
      'index_revision': 'index',
      'entries': <Object>[],
    },
    'list_git_branches' => {
      'entries': [
        {'name': 'feature', 'current': false},
      ],
    },
    'read_git_log' => {
      'entries': [
        {'id': 'commit', 'message': 'Initial commit'},
      ],
    },
    _ => throw StateError('Unexpected command: $kind'),
  },
};

void main() {
  testWidgets('Git tabs load on demand and invalidate cached data on refresh', (
    tester,
  ) async {
    final calls = <String>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {
        'worktrees': [fixture.workspace],
      },
      command: (kind, data) async {
        calls.add(kind);
        expect(data!['worktree'], 'tree');
        await Future<void>.delayed(const Duration(milliseconds: 30));
        return response(kind);
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      const GitPage(hostId: 'node', worktreeId: 'tree'),
      app,
    );
    expect(calls, ['inspect_git']);
    expect(find.byType(FailureState), findsNothing);
    expect(find.text('main'), findsOneWidget);
    await tester.tap(find.text(tr('gitBranches')));
    await tester.pumpAndSettle();
    expect(find.text('feature'), findsOneWidget);
    expect(calls, ['inspect_git', 'list_git_branches']);
    await tester.tap(find.text(tr('gitHistory')));
    await tester.pumpAndSettle();
    expect(find.text('Initial commit'), findsOneWidget);
    expect(calls, ['inspect_git', 'list_git_branches', 'read_git_log']);
    await tester.tap(find.text(tr('gitBranches')));
    await tester.pumpAndSettle();
    host.notifyListeners();
    await tester.pumpAndSettle();
    expect(calls.length, 3);
    await tester.tap(find.byTooltip(tr('refresh')));
    await tester.pumpAndSettle();
    expect(calls.skip(3), ['inspect_git', 'list_git_branches']);
    await tester.tap(find.text(tr('gitHistory')));
    await tester.pumpAndSettle();
    expect(calls.last, 'read_git_log');
    expect(calls.length, 6);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets(
    'pending tab reads are not duplicated and stop updating after exit',
    (tester) async {
      final pending = Completer<Map<String, dynamic>>();
      final calls = <String>[];
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        snapshot: {
          'worktrees': [fixture.workspace],
        },
        command: (kind, data) async {
          calls.add(kind);
          return kind == 'list_git_branches' ? pending.future : response(kind);
        },
      );
      final app = AppSession.test(hosts: [host]);
      await fixture.mount(
        tester,
        const GitPage(hostId: 'node', worktreeId: 'tree'),
        app,
      );
      await tester.tap(find.text(tr('gitBranches')));
      await tester.pump();
      expect(calls, ['inspect_git', 'list_git_branches']);
      host.notifyListeners();
      await tester.pump();
      final refresh = tester.widget<RoundButton>(
        find.byWidgetPredicate(
          (widget) => widget is RoundButton && widget.icon == 'refresh',
        ),
      );
      expect(refresh.onPressed, isNull);
      await tester.tap(find.text(tr('gitBranches')), warnIfMissed: false);
      await tester.pump();
      expect(calls.length, 2);
      await tester.pumpWidget(const SizedBox());
      pending.complete(response('list_git_branches'));
      await tester.pumpAndSettle();
      expect(calls.length, 2);
      expect(tester.takeException(), isNull);
      app.dispose();
    },
  );

  testWidgets('a busy Git read can be retried without reconnecting', (
    tester,
  ) async {
    var busy = true;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {
        'worktrees': [fixture.workspace],
      },
      command: (kind, data) async {
        if (busy) throw const CommandFailure('busy');
        return response(kind);
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      const GitPage(hostId: 'node', worktreeId: 'tree'),
      app,
    );
    expect(host.connected, isTrue);
    expect(find.text(tr('failureBusy')), findsOneWidget);
    busy = false;
    await tester.tap(find.widgetWithText(FilledButton, tr('retry')));
    await tester.pumpAndSettle();
    expect(find.byType(FailureState), findsNothing);
    expect(find.text('main'), findsOneWidget);
    expect(host.connected, isTrue);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });
}
