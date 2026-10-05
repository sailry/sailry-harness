import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/commands.dart';
import 'support/commands.dart';
import 'package:sailry_bridge/api/conversation.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/question.dart';
import 'package:sailry_mobile/features/conversations/live/queue.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/features/conversations/user_message.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:sailry_mobile/ui/empty_state.dart';
import 'package:sailry_mobile/ui/failure_state.dart';

class UpdatesFixture implements ConversationUpdates {
  final _values = StreamController<String>();
  late final _iterator = StreamIterator(_values.stream);
  bool closed = false;
  int older = 0;

  void emit(Map<String, dynamic> value) => _values.add(jsonEncode(value));
  void fail(Object error) => _values.addError(error);
  @override
  Future<String> next() async {
    if (await _iterator.moveNext()) return _iterator.current;
    throw StateError('closed');
  }

  @override
  Future<void> close() async {
    if (closed) return;
    closed = true;
    unawaited(_values.close());
  }

  @override
  Future<void> loadOlder() async {
    older++;
  }

  @override
  void dispose() {}
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class ConnectionFixture implements Connection {
  final commands = CommandFixture();
  String? commandSession;
  @override
  Future<CommandUpdates> watchCommands({required String session}) async {
    commandSession = session;
    return commands;
  }

  var updates = UpdatesFixture();
  final List<String> watched = [];
  final List<String> reconciled = [];
  @override
  Future<ConversationUpdates> watchConversation({
    required String session,
  }) async {
    watched.add(session);
    return updates;
  }

  @override
  Future<String> execute({required String request}) async {
    reconciled.add(request);
    return jsonEncode({
      'Ok': {
        'kind': 'queued_turn',
        'data': {'id': 'turn'},
      },
    });
  }

  @override
  Future<void> close() => updates.close();
  @override
  void dispose() {}
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Map<String, dynamic> session({String id = 'session', int revision = 7}) => {
  'id': id,
  'project': 'project',
  'worktree': 'tree',
  'revision': revision,
  'archived': false,
  'activity': {
    'title': 'Actual task',
    'run': null,
    'waiting': null,
    'queued': 0,
  },
  'config': {
    'provider': 'provider',
    'model': 'actual-model',
    'permission': 'ask',
    'mode': 'code',
    'effort': 'default',
    'credential': null,
  },
};

Map<String, dynamic> view({
  String status = 'completed',
  List<Map<String, dynamic>> entries = const [],
  List<Map<String, dynamic>> drafts = const [],
  List<Map<String, dynamic>> calls = const [],
  Map<String, dynamic>? queue,
  bool connected = true,
}) => {
  'connected': connected,
  'calls': calls,
  'snapshot': {
    'page': {
      'runs': [
        {'turn': 'turn', 'status': status},
      ],
      'entries': entries,
      'queue': queue ?? {'revision': 0, 'paused': false, 'items': []},
    },
    'drafts': drafts,
  },
};

Map<String, dynamic> entry(String id, String author, String text) => {
  'id': id,
  'turn': 'turn',
  'author': author,
  'parts': [
    {'kind': 'text', 'data': text},
  ],
};

Future<void> mount(WidgetTester tester, Widget page, AppSession app) async {
  tester.platformDispatcher.accessibilityFeaturesTestValue =
      const FakeAccessibilityFeatures(disableAnimations: true);
  addTearDown(tester.platformDispatcher.clearAccessibilityFeaturesTestValue);
  tester.view.physicalSize = const Size(430, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    SessionScope(
      session: app,
      child: MaterialApp(theme: SailryTheme.of(Brightness.light), home: page),
    ),
  );
  await tester.pump();
}

void main() {
  testWidgets('header ports use the captured host and session', (tester) async {
    final connection = ConnectionFixture();
    connection.commands.emit({'connected': true, 'items': []});
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (_, _) async => {},
    );
    final app = AppSession.test(hosts: [host]);
    await mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: session(),
      ),
      app,
    );
    connection.updates.emit(view());
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('conversation-ports')));
    await tester.pumpAndSettle();
    expect(connection.commandSession, 'session');
    expect(find.text(tr('resourceSessionServices')), findsOneWidget);
    expect(find.text(tr('resourceNoServices')), findsOneWidget);
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    expect(connection.commands.closed, isTrue);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('centered recovery with collapsed diagnostics', (tester) async {
    final connection = ConnectionFixture();
    final record = session();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [record],
      },
      command: (_, _) async => {},
    );
    final app = AppSession.test(hosts: [host]);
    await mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: record,
      ),
      app,
    );
    await tester.pump();
    final spinner = find.byType(CircularProgressIndicator);
    expect(spinner, findsOneWidget);
    expect(find.text(tr('conversationLoading')), findsNothing);
    expect(tester.getCenter(spinner).dx, 215);
    expect(tester.getCenter(spinner).dy, inInclusiveRange(350, 500));
    connection.updates.emit({
      'connected': false,
      'error': {'code': 'unavailable', 'message': 'Host unavailable'},
    });
    await tester.pump();
    await tester.pump();
    expect(spinner, findsNothing);
    expect(find.byType(MaterialBanner), findsNothing);
    expect(find.byType(FailureState), findsNothing);
    expect(find.text('Host unavailable'), findsNothing);
    expect(tester.getCenter(find.text(tr('conversationReconnecting'))).dx, 215);
    expect(
      tester.getCenter(find.text(tr('conversationReconnecting'))).dy,
      inInclusiveRange(350, 540),
    );
    await tester.tap(find.text(tr('conversationErrorDetails')));
    await tester.pumpAndSettle();
    expect(find.text('Host unavailable'), findsOneWidget);
    connection.updates.emit(
      view(entries: [entry('answer', 'assistant', 'Recovered answer')]),
    );
    await tester.pump();
    await tester.pump();
    expect(find.byType(EmptyState), findsNothing);
    expect(find.text(tr('conversationReconnecting')), findsNothing);
    expect(find.text('Host unavailable'), findsNothing);
    expect(find.text('Recovered answer'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('header inset and keyboard anchoring', (tester) async {
    tester.view.padding = const FakeViewPadding(top: 44, bottom: 24);
    addTearDown(tester.view.resetPadding);
    final connection = ConnectionFixture();
    final record = session();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [record],
      },
      command: (_, _) async => {},
    );
    final app = AppSession.test(hosts: [host]);
    await mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: record,
      ),
      app,
    );
    connection.updates.emit(
      view(
        entries: [
          for (var i = 0; i < 24; i++) entry('entry-$i', 'user', 'Message $i'),
        ],
      ),
    );
    await tester.pumpAndSettle();
    final bubbles = tester.widgetList<UserBubble>(find.byType(UserBubble));
    expect(bubbles, hasLength(24));
    expect(bubbles.every((bubble) => bubble.attachment == null), isTrue);
    final bubble = find
        .descendant(
          of: find.byType(UserBubble).last,
          matching: find.byType(Material),
        )
        .first;
    expect(
      tester.getSize(bubble).height -
          tester.getSize(find.text('Message 23')).height,
      closeTo(20, .1),
    );
    final list = find.byType(ListView).first;
    final controller = tester.widget<ListView>(list).controller!;
    expect(controller.position.extentBefore, 0);
    final header = find.byTooltip(tr('modelPicker'));
    final headerBounds = tester.getRect(header);
    expect(tester.getRect(list).top, 0);
    controller.jumpTo(controller.position.maxScrollExtent);
    await tester.pumpAndSettle();
    expect(
      tester.getTopLeft(find.text('Message 0')).dy,
      greaterThan(headerBounds.bottom + 16),
    );
    final oldestOffset = controller.offset;
    // The gradient must not intercept dragging between the header buttons.
    await tester.dragFrom(
      Offset(215, headerBounds.center.dy),
      const Offset(0, -100),
    );
    await tester.pumpAndSettle();
    expect(controller.offset, lessThan(oldestOffset));
    expect(tester.getRect(header), headerBounds);
    expect(
      tester.getTopLeft(find.text('Message 0')).dy,
      lessThan(headerBounds.bottom),
    );
    controller.jumpTo(0);
    await tester.pumpAndSettle();
    final input = find.byKey(const ValueKey('composer-input'));
    final inputBounds = tester.getRect(input);
    final viewportBottom = tester.getRect(list).bottom;
    expect(viewportBottom, tester.view.physicalSize.height);
    final latestBottom = tester.getBottomLeft(find.text('Message 23')).dy;
    await tester.enterText(find.byType(TextField), 'One\nTwo\nThree\nFour');
    await tester.pumpAndSettle();
    final expandedInput = tester.getRect(input);
    expect(expandedInput.top, lessThan(inputBounds.top));
    expect(tester.getRect(list).bottom, viewportBottom);
    expect(
      latestBottom - tester.getBottomLeft(find.text('Message 23')).dy,
      closeTo(inputBounds.top - expandedInput.top, 1),
    );
    controller.jumpTo(100);
    await tester.pumpAndSettle();
    // The input overlays history; the fading side gutter stays scrollable.
    await tester.dragFrom(
      Offset(4, expandedInput.center.dy),
      const Offset(0, 50),
    );
    await tester.pumpAndSettle();
    expect(controller.offset, greaterThan(100));
    await tester.enterText(find.byType(TextField), '');
    controller.jumpTo(0);
    await tester.pumpAndSettle();
    final before = tester.getBottomLeft(find.text('Message 23')).dy;
    tester.view.viewInsets = const FakeViewPadding(bottom: 280);
    addTearDown(tester.view.resetViewInsets);
    await tester.pumpAndSettle();
    final after = tester.getBottomLeft(find.text('Message 23')).dy;
    expect(before - after, closeTo(280, 1));
    expect(
      after,
      lessThan(
        tester.getTopLeft(find.byKey(const ValueKey('composer-input'))).dy,
      ),
    );
    controller.jumpTo(360);
    await tester.pumpAndSettle();
    tester.view.viewInsets = const FakeViewPadding(bottom: 320);
    await tester.pumpAndSettle();
    expect(controller.offset, 360);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  });

  testWidgets('empty list without a connected Node', (tester) async {
    final app = AppSession.test();
    await mount(tester, const TasksPage(), app);
    expect(find.text(tr('hostConnectPrompt')), findsOneWidget);
    expect(find.text(tr('approveTitle')), findsNothing);
    expect(find.text('sailry-web'), findsNothing);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  for (final binding in ['local', 'remote']) {
    testWidgets('$binding snapshots and drafts survive failures', (
      tester,
    ) async {
      final connection = ConnectionFixture();
      final commands = <(String, Map<String, dynamic>?)>[];
      var reject = true;
      final record = session();
      final host = HostConnection.test(
        id: binding,
        label: binding,
        connection: connection,
        snapshot: {
          'sessions': [record],
        },
        command: (kind, data) async {
          commands.add((kind, data));
          if (kind == 'submit_turn' && reject) {
            throw const CommandFailure('revision_conflict');
          }
          return {
            'kind': 'queued_turn',
            'data': {'id': 'next'},
          };
        },
      );
      final other = HostConnection.test(
        id: '$binding-other',
        label: 'Other Node',
        command: (_, _) async => throw StateError('wrong execution Node'),
      );
      final app = AppSession.test(hosts: [host, other]);
      await mount(
        tester,
        LiveConversationPage(
          host: host,
          sessionId: 'session',
          initialSession: record,
        ),
        app,
      );
      connection.updates.emit(
        view(
          status: 'running',
          entries: [entry('user', 'user', 'Canonical request')],
          drafts: [entry('draft', 'assistant', 'Streaming output')],
        ),
      );
      await tester.pump();
      await tester.pump();
      expect(connection.watched, ['session']);
      app.selectHost(other.id);
      await tester.pump();
      expect(find.text('Canonical request'), findsOneWidget);
      expect(find.text('Streaming output'), findsOneWidget);
      await tester.enterText(find.byType(TextField), 'Keep this unsent draft');
      await tester.tap(find.byTooltip(tr('enqueue')));
      await tester.pump();
      expect(find.text('Keep this unsent draft'), findsOneWidget);
      expect(commands.last.$2?['expected_revision'], 7);
      expect(commands.last.$2?['message'], {
        'text': 'Keep this unsent draft',
        'attachments': [],
      });
      expect(find.text(tr('conversationConflict')), findsOneWidget);
      connection.updates.emit({
        ...view(
          connected: false,
          status: 'running',
          entries: [entry('user', 'user', 'Canonical request')],
          drafts: [entry('draft', 'assistant', 'Streaming output')],
        ),
        'error': {'code': 'unavailable', 'message': 'Connection lost'},
      });
      await tester.pump();
      await tester.pump();
      expect(find.text(tr('conversationReconnecting')), findsOneWidget);
      expect(find.byType(FailureState), findsNothing);
      expect(find.text('Canonical request'), findsOneWidget);
      expect(find.text('Streaming output'), findsOneWidget);
      expect(find.text('Keep this unsent draft'), findsOneWidget);
      expect(find.text('Connection lost'), findsNothing);
      await tester.tap(find.text(tr('conversationErrorDetails')));
      await tester.pumpAndSettle();
      expect(find.text('Connection lost'), findsOneWidget);
      tester.view.physicalSize = const Size(320, 420);
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      tester.view.physicalSize = const Size(430, 900);
      await tester.pumpAndSettle();
      expect(connection.watched, ['session']);
      expect(commands.where((call) => call.$1 == 'submit_turn'), hasLength(1));
      connection.updates.emit(
        view(
          entries: [
            entry('user', 'user', 'Canonical request'),
            entry('answer', 'assistant', 'Committed answer'),
          ],
        ),
      );
      await tester.pump();
      await tester.pump();
      expect(find.text('Streaming output'), findsNothing);
      expect(find.text(tr('conversationReconnecting')), findsNothing);
      expect(find.text('Connection lost'), findsNothing);
      expect(find.text('Committed answer'), findsOneWidget);
      expect(find.text('Keep this unsent draft'), findsOneWidget);
      reject = false;
      await tester.tap(find.byTooltip(tr('send')));
      await tester.pump();
      expect(find.text('Keep this unsent draft'), findsNothing);
      expect(commands.where((call) => call.$1 == 'submit_turn'), hasLength(2));
      await tester.pump(const Duration(seconds: 3));
      await tester.pumpAndSettle();
      await tester.pumpWidget(const SizedBox());
      await tester.pump();
      expect(connection.updates.closed, isTrue);
      app.dispose();
    });
  }

  testWidgets('observer retry shows the reason without sending', (
    tester,
  ) async {
    final connection = ConnectionFixture();
    final record = session();
    var commands = 0;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [record],
      },
      command: (_, _) async {
        commands++;
        return {};
      },
    );
    final app = AppSession.test(hosts: [host]);
    await mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: record,
        initialDraft: 'Unsent draft',
      ),
      app,
    );
    connection.updates.emit({
      'connected': false,
      'error': {'code': 'invalid_request', 'message': 'History is unavailable'},
    });
    await tester.pump();
    connection.updates.fail(StateError('observer closed'));
    await tester.pump();
    await tester.pump();
    expect(find.byType(FailureState), findsOneWidget);
    expect(find.text(tr('conversationReconnecting')), findsNothing);
    expect(find.text('History is unavailable'), findsNothing);
    expect(find.text('Unsent draft'), findsOneWidget);
    await tester.tap(find.text(tr('conversationErrorDetails')));
    await tester.pumpAndSettle();
    expect(find.text('History is unavailable'), findsOneWidget);
    expect(find.text('Bad state: observer closed'), findsNothing);
    connection.updates = UpdatesFixture();
    await tester.tap(find.widgetWithText(FilledButton, tr('retry')));
    await tester.pump();
    connection.updates.emit(
      view(entries: [entry('answer', 'assistant', 'Restored')]),
    );
    await tester.pump();
    await tester.pump();
    expect(find.byType(FailureState), findsNothing);
    expect(find.text('Restored'), findsOneWidget);
    expect(find.text('Unsent draft'), findsOneWidget);
    expect(connection.watched, ['session', 'session']);
    expect(commands, 0);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('unknown send reuses the original request', (tester) async {
    final connection = ConnectionFixture();
    var submissions = 0;
    final record = session();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [record],
      },
      command: (kind, data) async {
        submissions++;
        throw const CommandFailure(
          'outcome_unknown',
          request: 'same-durable-request',
        );
      },
    );
    final app = AppSession.test(hosts: [host]);
    await mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: record,
      ),
      app,
    );
    connection.updates.emit(view());
    await tester.pump();
    await tester.pump();
    await tester.enterText(find.byType(TextField), 'Do this once');
    await tester.tap(find.byTooltip(tr('send')));
    await tester.pump();
    expect(find.text('Do this once'), findsOneWidget);
    await tester.tap(find.text(tr('conversationCheckResult')));
    await tester.pump();
    expect(connection.reconciled, ['same-durable-request']);
    expect(submissions, 1);
    expect(find.text('Do this once'), findsNothing);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('approval preserves shared call identity', (tester) async {
    final connection = ConnectionFixture();
    final commands = <(String, Map<String, dynamic>?)>[];
    final record = session();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [record],
      },
      command: (kind, data) async {
        commands.add((kind, data));
        return {'kind': 'approval', 'data': {}};
      },
    );
    final app = AppSession.test(hosts: [host]);
    await mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: record,
      ),
      app,
    );
    connection.updates.emit(
      view(
        status: 'running',
        entries: [
          {
            'id': 'call-entry',
            'turn': 'turn',
            'author': 'assistant',
            'parts': [
              {
                'kind': 'tool_call',
                'data': {
                  'name': 'write_file',
                  'arguments': {'path': 'draft.txt'},
                },
              },
            ],
          },
        ],
        calls: [
          {
            'name': 'write_file',
            'turn': 'turn',
            'source': {'entry': 'call-entry', 'index': 0},
            'state': 'waiting',
            'approval': {'id': 'approval-id', 'state': 'pending'},
          },
        ],
      ),
    );
    await tester.pump();
    await tester.pump();
    await tester.tap(find.text(tr('allowShort')));
    await tester.pump();
    expect(commands.single.$1, 'resolve_approval');
    expect(commands.single.$2, {
      'session': 'session',
      'approval': 'approval-id',
      'decision': 'approve',
    });
    await tester.tap(find.byTooltip(tr('stop')));
    await tester.pump();
    expect(commands.last.$1, 'stop_turn');
    expect(commands.last.$2, {'turn': 'turn'});
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('completed work folds with answer visible', (tester) async {
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async => {},
    );
    final app = AppSession.test(hosts: [host]);
    final projection = view(
      entries: [
        {
          'id': 'thought',
          'turn': 'turn',
          'author': 'assistant',
          'parts': [
            {'kind': 'thinking', 'data': 'Reasoning content'},
          ],
        },
        {
          'id': 'tool',
          'turn': 'turn',
          'author': 'assistant',
          'parts': [
            {
              'kind': 'tool_call',
              'data': {
                'name': 'read_file',
                'arguments': {'path': 'source.txt'},
              },
            },
            {
              'kind': 'tool_result',
              'data': {
                'result': {'text': 'Exact tool output'},
                'images': [],
              },
            },
          ],
        },
        entry('answer', 'assistant', 'Final answer stays visible'),
      ],
      calls: [
        {
          'name': 'read_file',
          'turn': 'turn',
          'source': {'entry': 'tool', 'index': 0},
          'response': {'entry': 'tool', 'index': 1},
          'state': 'returned',
        },
      ],
    );
    await mount(
      tester,
      Scaffold(
        body: SingleChildScrollView(
          child: LiveTimeline(
            view: projection,
            session: session(),
            host: host,
            command: (_, _) async => {},
          ),
        ),
      ),
      app,
    );
    expect(find.text('Final answer stays visible'), findsOneWidget);
    expect(find.text(tr('thought')), findsNothing);
    await tester.tap(find.text(tr('completed')));
    await tester.pumpAndSettle();
    expect(find.text(tr('thought')), findsOneWidget);
    await tester.tap(find.text(tr('thought')));
    await tester.pumpAndSettle();
    expect(find.text('Reasoning content'), findsOneWidget);
    expect(find.text('Final answer stays visible'), findsOneWidget);
    await tester.tap(find.textContaining(tr('tool_read_file')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Exact tool output'), findsOneWidget);
    // Nested disclosure booleans and selectable-text scroll offsets must not
    // share PageStorage identities when the process subtree is recreated.
    await tester.tap(find.text(tr('completed')));
    await tester.pumpAndSettle();
    expect(find.text('Final answer stays visible'), findsOneWidget);
    expect(find.textContaining('Exact tool output'), findsNothing);
    await tester.tap(find.text(tr('completed')));
    await tester.pumpAndSettle();
    expect(find.textContaining('Exact tool output'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('initial draft sends once after subscription', (tester) async {
    final connection = ConnectionFixture();
    final calls = <Map<String, dynamic>?>[];
    final record = session();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      snapshot: {
        'sessions': [record],
      },
      command: (kind, data) async {
        calls.add(data);
        return {'kind': 'queued_turn', 'data': {}};
      },
    );
    final app = AppSession.test(hosts: [host]);
    await mount(
      tester,
      LiveConversationPage(
        host: host,
        sessionId: 'session',
        initialSession: record,
        initialDraft: 'New task input',
        initialAttachments: [
          (
            attachment: {
              'id': 'first-file',
              'spec': {'name': 'brief.txt'},
            },
            preview: null,
          ),
        ],
        submitInitial: true,
      ),
      app,
    );
    expect(calls, isEmpty);
    connection.updates.emit(view());
    await tester.pump();
    await tester.pump();
    connection.updates.emit(view(status: 'running'));
    await tester.pump();
    await tester.pump();
    expect(calls, hasLength(1));
    expect(calls.single?['message'], {
      'text': 'New task input',
      'attachments': ['first-file'],
    });
    expect(find.text('New task input'), findsNothing);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('queued edits retain full input, attachments, and revision', (
    tester,
  ) async {
    final calls = <(String, Map<String, dynamic>)>[];
    final updates = ValueNotifier(
      view(
        queue: {
          'revision': 12,
          'paused': true,
          'items': [
            {
              'turn': 'queued',
              'revision': 3,
              'preview': 'Truncated…',
              'truncated': true,
            },
          ],
        },
      ),
    );
    final app = AppSession.test();
    await mount(
      tester,
      Scaffold(
        body: LiveQueue(
          view: updates,
          session: 'session',
          command: (kind, data) async {
            calls.add((kind, data));
            return {
              'kind': 'queued_message',
              'data': {
                'revision': 3,
                'message': {
                  'text': 'Full queued message',
                  'attachments': ['file'],
                  'references': [
                    {'kind': 'file'},
                  ],
                },
              },
            };
          },
        ),
      ),
      app,
    );
    await tester.tap(find.text('Truncated…'));
    await tester.pumpAndSettle();
    expect(find.text('Full queued message'), findsOneWidget);
    await tester.enterText(find.byType(TextField), 'Edited full message');
    await tester.tap(find.text(tr('save')));
    await tester.pumpAndSettle();
    expect(calls.last.$1, 'edit_queued_turn');
    expect(calls.last.$2, {
      'turn': 'queued',
      'expected_revision': 3,
      'message': {
        'text': 'Edited full message',
        'attachments': ['file'],
        'references': [
          {'kind': 'file'},
        ],
      },
    });
    await tester.tap(find.text(tr('resumeQueue')));
    await tester.pump();
    expect(calls.last.$1, 'set_queue_paused');
    expect(calls.last.$2, {
      'session': 'session',
      'expected_revision': 12,
      'paused': false,
    });
    await tester.pumpWidget(const SizedBox());
    updates.dispose();
    app.dispose();
  });

  testWidgets('question choices send protocol indices', (tester) async {
    final calls = <Map<String, dynamic>>[];
    final app = AppSession.test();
    await mount(
      tester,
      Scaffold(
        body: ConversationQuestion(
          spec: {
            'prompt': 'Select work',
            'input': {
              'kind': 'choice',
              'options': ['Code', 'Tests'],
              'multiple': true,
              'allow_other': true,
            },
          },
          question: {'id': 'question'},
          session: session(),
          command: (kind, data) async {
            calls.add(data);
            return {'kind': 'question', 'data': {}};
          },
        ),
      ),
      app,
    );
    await tester.tap(find.text('Tests'));
    await tester.pump();
    await tester.enterText(find.byType(TextField), 'Documentation');
    await tester.tap(find.text(tr('conversationSubmit')));
    await tester.pump();
    expect(calls.single['response'], {
      'kind': 'answer',
      'data': {
        'kind': 'choices',
        'data': {
          'selected': [1],
          'other': 'Documentation',
        },
      },
    });
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });
}
