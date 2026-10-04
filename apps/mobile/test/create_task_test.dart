import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/create.dart';
import 'package:sailry_mobile/features/conversations/message_composer.dart';
import 'package:sailry_mobile/features/conversations/live/model_controls.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'live_conversations_test.dart' as live;

class RecoveredCreation extends live.ConnectionFixture {
  late Map<String, dynamic> created;
  @override
  Future<String> execute({required String request}) async {
    reconciled.add(request);
    return jsonEncode({
      'Ok': {'kind': 'session', 'data': created},
    });
  }
}

Map<String, dynamic> provider(String id, {bool enabled = true}) => {
  'id': id,
  'name': 'Provider $id',
  'enabled': enabled,
  'credential': {
    'node': List.filled(32, id == 'a' ? 1 : 2),
    'id': 'credential-$id',
  },
  'default_model': 'missing-model',
  'models': [
    {
      'id': '$id-first',
      'default_effort': 'default',
      'efforts': ['high'],
    },
    {
      'id': '$id-second',
      'default_effort': 'high',
      'efforts': ['high'],
    },
  ],
};

Map<String, dynamic> snapshot(
  String id, {
  Map<String, dynamic>? config,
  List<Map<String, dynamic>>? providers,
}) => {
  'projects': [
    {'id': 'project-$id', 'name': 'Project $id'},
  ],
  'worktrees': [
    {'id': 'tree-$id', 'project': 'project-$id', 'path': '/project/$id'},
  ],
  'providers': providers ?? [provider(id)],
  'defaults': {'revision': 4, 'config': config},
};

Future<void> open(
  WidgetTester tester,
  AppSession app, {
  String draft = 'First real task',
}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(320, 740);
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    SessionScope(
      session: app,
      child: MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: const TasksPage(),
      ),
    ),
  );
  await tester.pumpAndSettle();
  await tester.tap(find.byTooltip(tr('newTask')));
  await tester.pumpAndSettle();
  expect(find.byType(NewConversationPage), findsOneWidget);
  expect(find.byType(BottomSheet), findsNothing);
  if (draft.isNotEmpty) await tester.enterText(find.byType(TextField), draft);
  await tester.pumpAndSettle();
}

Future<void> choose(WidgetTester tester, String label, String choice) async {
  if (label == 'model') {
    final field = find.byKey(const ValueKey('choose-model'));
    await tester.ensureVisible(field);
    await tester.tap(field);
    await tester.pumpAndSettle();
  }
  await tester.ensureVisible(find.text(choice).last);
  await tester.tap(find.text(choice).last);
  await tester.pumpAndSettle();
}

Future<void> submit(WidgetTester tester, {bool awaitingView = false}) async {
  final button = find.byTooltip(tr('send'));
  await tester.ensureVisible(button);
  await tester.pumpAndSettle();
  expect(
    tester.widget<MessageComposer>(find.byType(MessageComposer)).enabled,
    isTrue,
  );
  await tester.tap(button);
  if (awaitingView) {
    await tester.pump();
    await tester.pump();
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
  } else {
    await tester.pumpAndSettle();
  }
}

Map<String, dynamic> admitted(Map<String, dynamic> data) => {
  'id': 'created',
  'project': data['project'],
  'worktree': data['worktree'],
  'revision': 1,
  'config': data['config'],
  'activity': {'title': ''},
};

void main() {
  testWidgets('welcome prompts edit without creating a session', (
    tester,
  ) async {
    final calls = <String>[];
    final host = HostConnection.test(
      id: 'host-a',
      label: 'Host',
      snapshot: snapshot('a'),
      command: (kind, _) async {
        calls.add(kind);
        return {};
      },
    );
    final app = AppSession.test(hosts: [host]);
    await open(tester, app, draft: '');
    expect(find.text(tr('brand')), findsOneWidget);
    final explore = find.byKey(const ValueKey('welcome-Explore'));
    final build = find.byKey(const ValueKey('welcome-Build'));
    expect(tester.getTopLeft(explore).dy, tester.getTopLeft(build).dy);
    expect(
      tester.getTopLeft(explore).dx,
      lessThan(tester.getTopLeft(build).dx),
    );
    for (final name in ['Explore', 'Build', 'Review', 'Plan']) {
      await tester.enterText(find.byType(TextField), '');
      final card = find.byKey(ValueKey('welcome-$name'));
      await tester.ensureVisible(card);
      await tester.tap(card);
      await tester.pumpAndSettle();
      final editor = tester.widget<TextField>(find.byType(TextField));
      expect(editor.controller!.text, tr('welcome${name}Prompt'));
      expect(
        editor.controller!.selection.baseOffset,
        editor.controller!.text.length,
      );
      expect(editor.focusNode!.hasFocus, isTrue);
      expect(calls, isEmpty);
    }
    await tester.enterText(find.byType(TextField), 'Keep my requirements');
    await tester.ensureVisible(explore);
    await tester.tap(explore);
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      'Keep my requirements\n\n${tr('welcomeExplorePrompt')}',
    );
    expect(calls, isEmpty);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });

  testWidgets('scrollable welcome in compact layouts', (tester) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetViewInsets);
    final host = HostConnection.test(
      id: 'host-a',
      label: 'Host',
      snapshot: snapshot('a'),
      command: (_, _) async => throw StateError('prompts must remain local'),
    );
    for (final (size, scale, keyboard, brightness) in [
      (const Size(320, 640), 1.0, 240.0, Brightness.dark),
      (const Size(600, 320), 1.0, 0.0, Brightness.light),
      (const Size(320, 640), 2.0, 0.0, Brightness.dark),
    ]) {
      tester.view.physicalSize = size;
      tester.view.viewInsets = FakeViewPadding(bottom: keyboard);
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(brightness),
          builder: (context, child) => MediaQuery(
            data: MediaQuery.of(
              context,
            ).copyWith(textScaler: TextScaler.linear(scale)),
            child: child!,
          ),
          home: NewConversationPage(host: host),
        ),
      );
      await tester.pumpAndSettle();
      final last = find.byKey(const ValueKey('welcome-Plan'));
      await tester.ensureVisible(last);
      await tester.pumpAndSettle();
      final composer = tester.getRect(find.byType(MessageComposer));
      expect(tester.getBottomRight(last).dy, lessThanOrEqualTo(composer.top));
      expect(composer.bottom, lessThanOrEqualTo(size.height - keyboard));
      await tester.tap(last);
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(find.byType(TextField)).controller!.text,
        tr('welcomePlanPrompt'),
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    }
    host.dispose();
  });

  testWidgets('workspace draft creates a session on first send', (
    tester,
  ) async {
    final connection = live.ConnectionFixture();
    final calls = <(String, Map<String, dynamic>?)>[];
    final data = snapshot('a');
    data['projects'] = [
      ...(data['projects'] as List),
      {'id': 'project-next', 'name': 'Next project'},
    ];
    data['worktrees'] = [
      ...(data['worktrees'] as List),
      {'id': 'next-main', 'project': 'project-next', 'path': '/next/main'},
      {
        'id': 'next-feature',
        'project': 'project-next',
        'path': '/next/feature',
      },
    ];
    final host = HostConnection.test(
      id: 'host-a',
      label: 'Host',
      snapshot: data,
      connection: connection,
      command: (kind, data) async {
        calls.add((kind, data));
        return kind == 'create_session'
            ? {'data': admitted(data!)}
            : {'data': {}};
      },
    );
    final app = AppSession.test(hosts: [host]);
    await open(tester, app);
    expect(calls, isEmpty);
    await tester.tap(find.byTooltip(tr('modelPicker')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('draft-project')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Next project').last);
    await tester.pumpAndSettle();
    expect(find.text('/next/main'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('draft-worktree')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('/next/feature'));
    await tester.pumpAndSettle();
    expect(calls, isEmpty);
    await tester.tapAt(const Offset(4, 80));
    await tester.pumpAndSettle();
    expect(find.text('First real task'), findsOneWidget);
    await submit(tester, awaitingView: true);
    expect(calls.single.$2!['project'], 'project-next');
    expect(calls.single.$2!['worktree'], 'next-feature');
    connection.updates.emit(live.view());
    await tester.pumpAndSettle();
    expect(calls.map((call) => call.$1), ['create_session', 'submit_turn']);
    expect(calls.last.$2!['message'], {
      'text': 'First real task',
      'attachments': [],
    });
    connection.updates.emit(live.view());
    await tester.pumpAndSettle();
    expect(calls.length, 2);
    await tester.tap(find.byTooltip(tr('modelPicker')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('draft-project')), findsNothing);
    expect(find.byKey(const ValueKey('draft-worktree')), findsNothing);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });

  testWidgets('uncertain creation preserves the draft and request', (
    tester,
  ) async {
    final connection = RecoveredCreation();
    var creates = 0;
    final host = HostConnection.test(
      id: 'host-a',
      label: 'Host',
      snapshot: snapshot('a'),
      connection: connection,
      command: (kind, data) async {
        if (kind == 'create_session') {
          creates++;
          connection.created = admitted(data!);
          throw const CommandFailure(
            'outcome_unknown',
            request: 'same-request',
          );
        }
        return {};
      },
    );
    final app = AppSession.test(hosts: [host]);
    await open(tester, app);
    await submit(tester);
    expect(find.text('First real task'), findsOneWidget);
    expect(
      tester.widget<MessageComposer>(find.byType(MessageComposer)).enabled,
      isFalse,
    );
    expect(
      find.widgetWithText(FilledButton, tr('conversationCheckResult')),
      findsOneWidget,
    );
    await tester.tap(find.text(tr('conversationCheckResult')));
    await tester.pump();
    await tester.pump();
    expect(creates, 1);
    expect(connection.reconciled, ['same-request']);
    expect(find.byType(LiveConversationPage), findsOneWidget);
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    connection.updates.emit(live.view());
    await tester.pumpAndSettle();
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });

  testWidgets('existing model with unchanged Node defaults', (tester) async {
    final calls = <(String, Map<String, dynamic>?)>[];
    final host = HostConnection.test(
      id: 'host-a',
      label: 'Node A',
      snapshot: snapshot('a'),
      command: (kind, data) async {
        calls.add((kind, data));
        expect(kind, 'create_session');
        return {'kind': 'session', 'data': admitted(data!)};
      },
    );
    final app = AppSession.test(hosts: [host]);
    await open(tester, app);
    expect(calls, isEmpty);
    await tester.tap(find.byTooltip(tr('modelPicker')));
    await tester.pumpAndSettle();
    expect(
      tester.widget<ModelControls>(find.byType(ModelControls)).config['model'],
      'a-first',
    );
    expect(find.text('missing-model'), findsNothing);
    await choose(tester, 'model', 'a-second');
    await choose(tester, 'conversationMode', tr('conversationPlan'));
    await choose(tester, 'conversationPermission', tr('conversationProject'));
    await tester.tapAt(const Offset(4, 80));
    await tester.pumpAndSettle();
    await submit(tester);
    expect(calls.length, 1);
    expect(calls.single.$2!['config'], {
      'provider': 'a',
      'model': 'a-second',
      'credential': provider('a')['credential'],
      'effort': 'high',
      'mode': 'plan',
      'permission': 'project',
    });
    expect((host.snapshot['defaults'] as Map)['config'], isNull);
    final page = tester.widget<LiveConversationPage>(
      find.byType(LiveConversationPage),
    );
    expect(page.host, same(host));
    expect(page.initialDraft, 'First real task');
    expect(page.submitInitial, isTrue);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });

  testWidgets('host defaults and admission stay captured', (tester) async {
    final config = {
      'provider': 'b',
      'model': 'b-second',
      'credential': provider('b')['credential'],
      'effort': {'budget': 2048},
      'mode': 'plan',
      'permission': 'full',
    };
    final reply = Completer<Map<String, dynamic>>();
    Map<String, dynamic>? requested;
    final first = HostConnection.test(
      id: 'host-a',
      label: 'Node A',
      snapshot: snapshot('a'),
      command: (_, _) async => throw StateError('wrong execution host'),
    );
    final second = HostConnection.test(
      id: 'host-b',
      label: 'Node B',
      snapshot: snapshot('b', config: config),
      command: (kind, data) {
        expect(kind, 'create_session');
        requested = data;
        return reply.future;
      },
    );
    final app = AppSession.test(hosts: [first, second]);
    app.selectHost(second.id);
    await open(tester, app);
    await tester.tap(find.byTooltip(tr('modelPicker')));
    await tester.pumpAndSettle();
    expect(find.text('Node A'), findsNothing);
    expect(
      tester.widget<ModelControls>(find.byType(ModelControls)).config['model'],
      'b-second',
    );
    expect(
      tester.widget<ModelControls>(find.byType(ModelControls)).config['effort'],
      {'budget': 2048},
    );
    expect(
      tester
          .widget<SegmentedButton<String>>(
            find.byKey(const ValueKey('configuration-permission')),
          )
          .selected,
      {'full'},
    );
    await tester.tapAt(const Offset(4, 80));
    await tester.pumpAndSettle();
    await submit(tester);
    expect(requested!['config'], config);
    expect(requested!['project'], 'project-b');
    expect(requested!['worktree'], 'tree-b');
    expect(
      tester.widget<MessageComposer>(find.byType(MessageComposer)).enabled,
      isFalse,
    );
    app.selectHost(first.id);
    second.snapshot = snapshot(
      'b',
      config: {...config, 'model': 'b-first', 'permission': 'ask'},
    );
    expect(requested!['config'], config);
    reply.complete({'kind': 'session', 'data': admitted(requested!)});
    await tester.pumpAndSettle();
    final page = tester.widget<LiveConversationPage>(
      find.byType(LiveConversationPage),
    );
    expect(page.host, same(second));
    expect(page.initialSession['config'], config);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });

  testWidgets('disabled and missing models block creation', (tester) async {
    final host = HostConnection.test(
      id: 'host-a',
      label: 'Node A',
      snapshot: snapshot(
        'a',
        config: {'provider': 'a', 'model': 'gone'},
        providers: [
          provider('a', enabled: false),
          {...provider('b'), 'models': []},
        ],
      ),
      command: (_, _) async =>
          throw StateError('unavailable model must not be submitted'),
    );
    final app = AppSession.test(hosts: [host]);
    await open(tester, app);
    expect(
      tester.widget<MessageComposer>(find.byType(MessageComposer)).sendEnabled,
      isFalse,
    );
    expect(find.text(tr('conversationNoModel')), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });
}
