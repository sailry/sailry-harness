import 'dart:async';
import 'dart:convert';

import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api.dart' show Controller;
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/conversation.dart';
import 'package:sailry_bridge/bridge.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/features/conversations/conversation_frame.dart';
import 'package:sailry_mobile/features/conversations/live/connection_status.dart';
import 'package:sailry_mobile/features/conversations/live/glass_chip.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/project_bar.dart';
import 'package:sailry_mobile/features/conversations/live/queue.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';
import 'package:sailry_mobile/features/conversations/live/timeline.dart';
import 'package:sailry_mobile/features/conversations/message_composer.dart';
import 'package:sailry_mobile/features/conversations/task_row.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart' show pushPage;
import 'package:sailry_mobile/ui/theme.dart';

Map<String, dynamic> conversation({
  String title = 'Conversation',
  String turn = 'first',
  int revision = 1,
  int attention = 1,
  bool unread = true,
}) => {
  'id': 'session',
  'title': title,
  'revision': revision,
  'activity': {
    'title': title,
    'attention': {'revision': attention, 'unread': unread},
    'run': {'turn': turn, 'sequence': 1, 'revision': 1, 'status': 'completed'},
  },
};

Map<String, dynamic> preview(String text) => {
  'data': {
    'page': {
      'revision': 1,
      'entries': [
        {
          'parts': [
            {'kind': 'text', 'data': text},
          ],
        },
      ],
    },
  },
};

Widget screen(
  AppSession session, {
  Widget home = const TasksPage(),
  GlobalKey<NavigatorState>? navigator,
}) => SessionScope(
  session: session,
  child: MaterialApp(
    navigatorKey: navigator,
    theme: SailryTheme.of(Brightness.light),
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    home: home,
  ),
);

Future<void> release(WidgetTester tester, AppSession session) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await session.close();
  session.dispose();
}

class _ConversationFeed extends Fake implements ConversationUpdates {
  final _views = StreamController<String>();
  late final _reader = StreamIterator(_views.stream);

  void publish(Map<String, dynamic> view) => _views.add(jsonEncode(view));

  @override
  Future<String> next() async {
    if (await _reader.moveNext()) return _reader.current;
    throw StateError('conversation closed');
  }

  @override
  Future<void> close() async => unawaited(_views.close());

  @override
  void dispose() {}
}

class _FailedConversation extends Fake implements Connection {
  final feed = _ConversationFeed();
  final watches = <String>[];
  final refreshes = <String>[];

  @override
  Future<ConversationUpdates> watchConversation({
    required String session,
    bool referenceRows = false,
    bool recent = false,
  }) async {
    watches.add(session);
    feed.publish({
      'connected': false,
      'error': {'code': 'wrong_target', 'message': 'Conversation mismatch'},
    });
    return feed;
  }

  @override
  Future<void> refreshConversation({required String session}) async {
    refreshes.add(session);
    feed.publish({
      'connected': true,
      'snapshot': {'page': <String, dynamic>{}},
    });
  }

  @override
  Future<void> close() async {}

  @override
  void dispose() {}
}

class _ManualConversationFeed extends Fake implements ConversationUpdates {
  Completer<String>? _pending;
  final toolSelections = <String>[];
  bool lateDelivery = false;
  int closes = 0;
  int disposals = 0;

  void publish(Map<String, dynamic> view) =>
      _pending!.complete(jsonEncode(view));
  void fail(Object error) => _pending!.completeError(error);

  @override
  Future<String> next() => (_pending = Completer<String>()).future;

  @override
  Future<void> trackTools({required String selection}) async {
    toolSelections.add(selection);
  }

  @override
  Future<void> close() async {
    closes++;
  }

  @override
  void dispose() {
    disposals++;
    final pending = _pending;
    if (!lateDelivery && pending != null && !pending.isCompleted) {
      pending.completeError(
        const BridgeFailure(code: 'cancelled', message: 'Observation closed'),
      );
    }
  }
}

class _LifecycleConnection extends Fake implements Connection {
  final watches = <String>[];
  final watchOptions = <({bool referenceRows, bool recent})>[];
  final feeds = <_ManualConversationFeed>[];

  @override
  Future<ConversationUpdates> watchConversation({
    required String session,
    bool referenceRows = false,
    bool recent = false,
  }) async {
    watches.add(session);
    watchOptions.add((referenceRows: referenceRows, recent: recent));
    final feed = _ManualConversationFeed();
    feeds.add(feed);
    return feed;
  }

  @override
  Future<void> close() async {}

  @override
  void dispose() {}
}

class _Directory extends Fake implements Controller {
  @override
  Future<List<String>> peers() async => [
    jsonEncode({'id': 'target'}),
  ];

  @override
  Future<void> close() async {}

  @override
  void dispose() {}
}

Finder _message(String text) => find.byWidgetPredicate(
  (widget) => widget is MarkdownContent && widget.data == text,
);

Map<String, dynamic> _transcript(
  String session,
  String body, {
  int count = 1,
  String? before,
}) {
  final turns = List.generate(count, (index) => 'turn-$index');
  Map<String, dynamic> part(String turn) => {
    'kind': 'text',
    'data': turn == turns.last ? body : 'History for $turn',
  };
  return {
    'connected': true,
    'snapshot': {
      'statistics': {'turns': count},
      'page': {
        'session': session,
        'revision': 1,
        'next_before': before,
        'runs': [
          for (final turn in turns)
            {'turn': turn, 'kind': 'task', 'status': 'completed'},
        ],
        'entries': [
          for (final turn in turns)
            {
              'id': turn,
              'turn': turn,
              'author': 'assistant',
              'parts': [part(turn)],
            },
        ],
      },
    },
    'timeline': turns,
    'segments': {
      for (final turn in turns)
        turn: [
          {
            'rows': [
              {
                'source': {'entry': turn, 'index': 0},
                'part': part(turn),
                'text_offset': 0,
                'text_last': true,
              },
            ],
          },
        ],
    },
  };
}

void main() {
  testWidgets('retry refreshes a non-recovering conversation in place', (
    tester,
  ) async {
    final connection = _FailedConversation();
    final initial = conversation(unread: false);
    final host = HostConnection.test(
      id: 'entry/target',
      label: 'Host',
      connection: connection,
      snapshot: {
        'sessions': [initial],
      },
    );
    final session = AppSession.test(hosts: [host]);
    await tester.pumpWidget(
      screen(
        session,
        home: LiveConversationPage(
          host: host,
          sessionId: 'session',
          initialSession: initial,
        ),
      ),
    );
    await tester.pumpAndSettle();
    final translate = tester.element(find.byType(LiveConversationPage)).tr;
    final retry = find.widgetWithText(FilledButton, translate('retry'));
    MessageComposer composer() => tester.widget(find.byType(MessageComposer));
    expect(retry, findsOneWidget);
    expect(composer().enabled, isFalse);
    expect(connection.watches, ['session']);
    expect(connection.refreshes, isEmpty);
    await tester.tap(retry);
    await tester.pumpAndSettle();
    expect(connection.refreshes, ['session']);
    expect(connection.watches, ['session']);
    expect(retry, findsNothing);
    expect(composer().enabled, isTrue);
    await release(tester, session);
  });

  testWidgets(
    'stacked forks release covered views and restore drafts and scroll',
    (tester) async {
      final connection = _LifecycleConnection();
      final parent = conversation(unread: false);
      final branches = [
        for (final id in ['first-fork', 'second-fork'])
          {...conversation(title: id, unread: false), 'id': id},
      ];
      var fork = 0;
      final host = HostConnection.test(
        id: 'entry/target',
        label: 'Host',
        connection: connection,
        snapshot: {
          'sessions': [parent, ...branches],
        },
        command: (kind, data) async {
          expect(kind, 'fork_conversation');
          expect(data?['session'], fork == 0 ? 'session' : 'first-fork');
          return {'data': branches[fork++]};
        },
      );
      final session = AppSession.test(hosts: [host]);
      final navigator = GlobalKey<NavigatorState>();
      await tester.pumpWidget(
        screen(
          session,
          navigator: navigator,
          home: LiveConversationPage(
            host: host,
            sessionId: 'session',
            initialSession: parent,
          ),
        ),
      );
      LiveTimeline timeline() => tester.widget(find.byType(LiveTimeline));
      TextEditingController draft() => tester
          .widget<MessageComposer>(find.byType(MessageComposer))
          .controller;
      Finder body(String id) =>
          find.byKey(ValueKey((host.id, id)), skipOffstage: false);
      Future<CupertinoPageRoute<void>> openFork(
        String id,
        String previousId,
        _ManualConversationFeed previousFeed,
      ) async {
        timeline().onTurnAction!('fork', 'turn-23');
        await tester.pump();
        await tester.pump();
        expect(connection.watches.last, id);
        final route = ModalRoute.of<void>(
          tester.element(
            find.byWidgetPredicate(
              (widget) =>
                  widget is LiveConversationPage && widget.sessionId == id,
            ),
          ),
        );
        expect(route, isA<CupertinoPageRoute<void>>());
        final cover = route! as CupertinoPageRoute<void>;
        expect(cover.opaque, isTrue);
        connection.feeds.last.publish(_transcript(id, '$id body', count: 24));
        await tester.pump(cover.transitionDuration ~/ 2);
        expect(cover.offstage, isFalse);
        expect(cover.animation!.status, AnimationStatus.forward);
        expect(previousFeed.closes, 0);
        expect(body(previousId), findsOneWidget);
        await tester.pumpAndSettle();
        expect(cover.animation!.status, AnimationStatus.completed);
        expect(previousFeed.closes, 1);
        expect(previousFeed.disposals, 1);
        expect(body(previousId), findsNothing);
        return cover;
      }

      Future<void> returnFromFork(
        CupertinoPageRoute<void> cover,
        String id,
        int priorWatches,
      ) async {
        navigator.currentState!.pop();
        await tester.pump();
        await tester.pump(cover.reverseTransitionDuration ~/ 2);
        expect(cover.animation!.status, AnimationStatus.reverse);
        expect(connection.watches, hasLength(priorWatches));
        expect(body(id), findsNothing);
        await tester.pump(cover.reverseTransitionDuration);
        await tester.pump();
        await tester.pump();
        expect(connection.watches, hasLength(priorWatches + 1));
        expect(connection.watches.last, id);
        expect(body(id), findsNothing);
      }

      final original = connection.feeds.single..lateDelivery = true;
      original.publish(_transcript('session', 'Original body', count: 24));
      await tester.pumpAndSettle();
      final parentDraft = draft()..text = 'Unsent parent text';
      final scroll = timeline().controller;
      scroll.jumpTo(180);
      await tester.pump();
      expect(scroll.offset, 180);

      final firstCover = await openFork('first-fork', 'session', original);
      final firstFork = connection.feeds.last..lateDelivery = true;
      final branchDraft = draft()..text = 'Unsent branch text';
      timeline().controller.jumpTo(20);

      final secondCover = await openFork(
        'second-fork',
        'first-fork',
        firstFork,
      );
      expect(body('session'), findsNothing);

      await returnFromFork(secondCover, 'first-fork', 3);
      firstFork.publish(_transcript('first-fork', 'Late branch body'));
      await tester.pump();
      expect(body('first-fork'), findsNothing);
      connection.feeds.last.publish(
        _transcript('first-fork', 'Fresh branch body', count: 24),
      );
      await tester.pumpAndSettle();
      expect(_message('Fresh branch body'), findsOneWidget);
      expect(timeline().controller.position.extentBefore, 0);
      expect(draft(), same(branchDraft));
      expect(draft().text, 'Unsent branch text');

      await returnFromFork(firstCover, 'session', 4);
      expect(connection.watchOptions, [
        for (final recent in [true, true, true, true, false])
          (referenceRows: true, recent: recent),
      ]);
      final restored = connection.feeds.last;
      restored.publish(
        _transcript('session', 'Fresh partial body', before: 'older'),
      );
      await tester.pumpAndSettle();
      expect(scroll.offset, lessThan(180));
      original.publish(_transcript('session', 'Delayed original callback'));
      await tester.pump();
      expect(_message('Fresh partial body'), findsOneWidget);
      expect(_message('Delayed original callback'), findsNothing);
      restored.publish(_transcript('session', 'Fresh full body', count: 24));
      await tester.pumpAndSettle();
      expect(_message('Delayed original callback'), findsNothing);
      expect(scroll.offset, 180);
      expect(draft(), same(parentDraft));
      expect(draft().text, 'Unsent parent text');
      await release(tester, session);
    },
  );

  testWidgets(
    'ordinary detail routes and sheets keep the conversation observer',
    (tester) async {
      final connection = _LifecycleConnection();
      final initial = conversation(unread: false);
      final host = HostConnection.test(
        id: 'target',
        label: 'Host',
        connection: connection,
      );
      final session = AppSession.test(hosts: [host]);
      final navigator = GlobalKey<NavigatorState>();
      await tester.pumpWidget(
        screen(
          session,
          navigator: navigator,
          home: LiveConversationPage(
            host: host,
            sessionId: 'session',
            initialSession: initial,
          ),
        ),
      );
      final feed = connection.feeds.single;
      feed.publish(_transcript('session', 'Initial body'));
      await tester.pumpAndSettle();

      unawaited(
        pushPage<void>(
          tester.element(find.byType(LiveConversationPage)),
          const Scaffold(body: Text('Ordinary detail')),
        ),
      );
      await tester.pumpAndSettle();
      feed.publish(_transcript('session', 'Body updated behind detail'));
      await tester.pumpAndSettle();
      expect(feed.closes, 0);
      navigator.currentState!.pop();
      await tester.pumpAndSettle();
      expect(_message('Body updated behind detail'), findsOneWidget);

      await tester.tap(find.byKey(const ValueKey('conversation-info')));
      await tester.pumpAndSettle();
      feed.publish(_transcript('session', 'Body updated behind sheet'));
      await tester.pumpAndSettle();
      expect(feed.closes, 0);
      navigator.currentState!.pop();
      await tester.pumpAndSettle();
      expect(_message('Body updated behind sheet'), findsOneWidget);
      expect(connection.watches, ['session']);
      await release(tester, session);
    },
  );

  testWidgets('the live queue leads the activity row on a narrow screen', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(320, 640);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    final connection = _LifecycleConnection();
    final initial = conversation(unread: false);
    final commands = <Map<String, dynamic>>[];
    final host = HostConnection.test(
      id: 'target',
      label: 'Host',
      connection: connection,
      command: (kind, data) async {
        expect(kind, 'set_queue_paused');
        commands.add(data!);
        return {};
      },
    );
    final session = AppSession.test(hosts: [host]);
    await tester.pumpWidget(
      screen(
        session,
        home: LiveConversationPage(
          host: host,
          sessionId: 'session',
          initialSession: initial,
        ),
      ),
    );
    expect(connection.watchOptions, [(referenceRows: true, recent: true)]);
    final feed = connection.feeds.single;
    const queued = 'Check the narrow layout';
    void publishQueue({required bool paused}) {
      final view = _transcript('session', 'Initial body');
      final Map<String, dynamic> page = view['snapshot']['page'];
      page['background'] = [
        {'id': 'task', 'state': 'running', 'kind': {'kind': 'command'}},
      ];
      page['queue'] = {
        'revision': paused ? 8 : 7,
        'paused': paused,
        'items': [
          {'turn': 'next', 'kind': 'task', 'revision': 1, 'preview': queued},
        ],
      };
      feed.publish(view);
    }

    publishQueue(paused: false);
    await tester.pumpAndSettle();
    final translate = tester.element(find.byType(LiveConversationPage)).tr;
    final row = find.byType(ComposerChipRow);
    expect(row, findsOneWidget);
    Finder inRow(Finder matching) =>
        find.descendant(of: row, matching: matching).hitTestable();
    final chip = inRow(find.byType(QueueChip));
    final background = inRow(find.text(translate('activityBackground')));
    expect(chip, findsOneWidget);
    expect(background, findsOneWidget);
    expect(tester.getCenter(chip).dx, lessThan(tester.getCenter(background).dx));

    await tester.tap(chip);
    await tester.pumpAndSettle();
    expect(find.text(queued), findsOneWidget);
    final pause = find
        .widgetWithText(OutlinedButton, translate('pauseQueue'))
        .hitTestable();
    expect(tester.widget<OutlinedButton>(pause).onPressed, isNotNull);
    await tester.tap(pause);
    await tester.pumpAndSettle();
    expect(commands, [
      {'session': 'session', 'expected_revision': 7, 'paused': true},
    ]);

    publishQueue(paused: true);
    await tester.pumpAndSettle();
    final resume = find.widgetWithText(OutlinedButton, translate('resumeQueue'));
    expect(tester.widget<OutlinedButton>(resume).onPressed, isNotNull);
    Navigator.of(tester.element(find.byType(LiveQueue))).pop();
    await tester.pumpAndSettle();
    expect(tester.widget<QueueChip>(chip).state, (
      count: 1,
      paused: true,
      guiding: false,
    ));
    expect(connection.watches, ['session']);
    expect(feed.closes, 0);
    await release(tester, session);
    expect(feed.closes, 1);
    expect(feed.disposals, 1);
  });

  for (final terminal in ['revoked', 'removed', 'terminated']) {
    testWidgets(
      'same owner survives unavailable but discards a $terminal view',
      (tester) async {
        final connection = _LifecycleConnection();
        final initial = conversation(unread: false);
        final host = HostConnection.test(
          id: 'entry/target',
          label: 'Routed host',
          connection: connection,
        );
        final direct = HostConnection.test(id: 'target', label: 'Direct host');
        final session = AppSession.test(
          hosts: [host, direct],
          controller: _Directory(),
        );
        await tester.pumpWidget(
          screen(
            session,
            home: LiveConversationPage(
              host: host,
              sessionId: 'session',
              initialSession: initial,
            ),
          ),
        );
        final feed = connection.feeds.single;
        feed.publish(_transcript('session', 'Owned transcript'));
        await tester.pumpAndSettle();
        expect(_message('Owned transcript'), findsOneWidget);
        expect(feed.toolSelections, isNotEmpty);
        expect(jsonDecode(feed.toolSelections.first), {
          'items': [],
          'retry': [],
        });
        expect(find.byKey(const ValueKey('app-toast')), findsNothing);

        const outage = {
          'code': 'unavailable',
          'message': 'Entry temporarily offline',
        };
        host
          ..connected = false
          ..error = outage
          ..notifyListeners();
        feed.publish({'connected': false, 'error': outage});
        // The reconnecting banner's spinner never settles.
        await tester.pump(const Duration(milliseconds: 300));
        expect(_message('Owned transcript'), findsOneWidget);
        expect(
          find.descendant(
            of: find.byType(ConnectionBanner),
            matching: find.text(
              tester
                  .element(find.byType(LiveConversationPage))
                  .tr('conversationReconnecting'),
            ),
          ),
          findsOneWidget,
        );
        expect(feed.closes, 0);

        host
          ..connected = true
          ..error = null
          ..notifyListeners();
        feed.publish(_transcript('session', 'Recovered transcript'));
        await tester.pumpAndSettle();
        expect(_message('Recovered transcript'), findsOneWidget);
        expect(connection.watches, ['session']);

        feed.lateDelivery = terminal != 'terminated';
        switch (terminal) {
          case 'revoked':
            host
              ..connected = false
              ..error = const {
                'code': 'permission_denied',
                'message': 'Route revoked',
              }
              ..notifyListeners();
          case 'removed':
            await session.refreshHosts(refreshConnections: false);
            expect(session.host(host.id), isNull);
            expect(session.host(direct.id), same(direct));
          case 'terminated':
            feed.fail(
              const BridgeFailure(
                code: 'cancelled',
                message: 'Observation ended',
              ),
            );
        }
        await tester.pumpAndSettle();
        expect(feed.closes, 1);
        expect(feed.disposals, 1);
        expect(find.byType(LiveTimeline), findsNothing);
        if (feed.lateDelivery) {
          feed.publish(_transcript('session', 'Late private transcript'));
          await tester.pumpAndSettle();
          expect(find.byType(LiveTimeline), findsNothing);
        }
        expect(connection.watches, ['session']);
        await release(tester, session);
      },
    );
  }

  testWidgets('task rows and filters use one host session lane projection', (
    tester,
  ) async {
    final child = {
      ...conversation(title: 'Delegated child'),
      'id': 'child',
      'delegation': {'session': 'session'},
    };
    final initial = [conversation(), child];
    final projected = <List<Object?>>[];
    var lane = 'running';
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {'sessions': initial},
      projectSessionLanes: (sessions) {
        projected.add(sessions);
        return {'session': lane};
      },
      command: (_, _) async => preview('Preview'),
    );
    final session = AppSession.test(hosts: [host]);
    await tester.pumpWidget(screen(session));
    await tester.pump();

    TaskRow row() => tester.widget(find.byType(TaskRow));
    final translate = tester.element(find.byType(TaskRow)).tr;
    expect(projected.single, same(initial));
    expect(find.text('Delegated child'), findsNothing);
    expect(find.text(translate('tasksActive')), findsOneWidget);
    expect(row().tone, StatusTone.running);
    expect(row().status, translate('running'));
    await tester.tap(find.text(translate('running')));
    await tester.pump();
    expect(find.text('Conversation'), findsOneWidget);
    await tester.tap(find.text(translate('all')));
    host.snapshot = {...host.snapshot, 'projects': []};
    host.notifyListeners();
    await tester.pump();
    expect(projected, hasLength(1));

    for (final (unread, next, tone) in [
      (true, 'failed', StatusTone.danger),
      (false, 'idle', null),
    ]) {
      lane = next;
      final record = conversation(unread: unread);
      final activity = record['activity'] as Map<String, dynamic>;
      (activity['run'] as Map<String, dynamic>)['status'] = 'failed';
      host.snapshot = {
        'sessions': [record],
      };
      host.notifyListeners();
      await tester.pump();
      expect(row().tone, tone);
    }
    expect(find.byKey(const ValueKey('task-status-dot')), findsNothing);
    expect(projected, hasLength(3));
    await release(tester, session);
  });

  testWidgets('read acknowledgements and titles do not reload history', (
    tester,
  ) async {
    var reads = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {
        'sessions': [conversation()],
      },
      command: (kind, _) async {
        expect(kind, 'read_conversation');
        reads++;
        return preview('Preview $reads');
      },
    );
    final session = AppSession.test(hosts: [host]);
    await tester.pumpWidget(screen(session));
    await tester.pump();
    expect(reads, 1);

    host.snapshot['sessions'] = [
      conversation(title: 'Renamed', revision: 2, attention: 2, unread: false),
    ];
    host.notifyListeners();
    await tester.pump();
    expect(find.text('Renamed'), findsOneWidget);
    expect(find.text('Preview 1'), findsOneWidget);
    expect(reads, 1);

    host.snapshot['sessions'] = [conversation(turn: 'second')];
    host.notifyListeners();
    await tester.pump();
    await tester.pump();
    expect(reads, 2);
    expect(find.text('Preview 2'), findsOneWidget);
    await release(tester, session);
  });

  testWidgets('covered tasks resume once after the return transition', (
    tester,
  ) async {
    var reads = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {
        'sessions': [conversation()],
      },
      command: (kind, _) async {
        expect(kind, 'read_conversation');
        reads++;
        return preview('Preview $reads');
      },
    );
    final session = AppSession.test(hosts: [host]);
    final navigator = GlobalKey<NavigatorState>();
    await tester.pumpWidget(screen(session, navigator: navigator));
    await tester.pumpAndSettle();
    final previous = tester.widget<ConversationTask>(
      find.byType(ConversationTask),
    );
    unawaited(
      navigator.currentState!.push(
        CupertinoPageRoute<void>(
          builder: (_) => const Scaffold(body: Text('Details')),
        ),
      ),
    );
    await tester.pumpAndSettle();

    for (final turn in ['second', 'third']) {
      host.snapshot['sessions'] = [
        conversation(title: 'Updated $turn', turn: turn),
      ];
      host.notifyListeners();
      await tester.pump();
    }
    expect(reads, 1);
    expect(
      tester.widget<ConversationTask>(
        find.byType(ConversationTask, skipOffstage: false),
      ),
      same(previous),
    );

    navigator.currentState!.pop();
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(reads, 1);
    await tester.pumpAndSettle();
    expect(reads, 2);
    expect(find.text('Updated third'), findsOneWidget);
    expect(find.text('Preview 2'), findsOneWidget);
    await release(tester, session);
  });

  testWidgets('a covered preview stops following large tool history', (
    tester,
  ) async {
    final pending = Completer<Map<String, dynamic>>();
    var reads = 0;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {
        'sessions': [conversation()],
      },
      command: (kind, _) async {
        expect(kind, 'read_conversation');
        return ++reads == 1 ? pending.future : preview('Latest message');
      },
    );
    final session = AppSession.test(hosts: [host]);
    final navigator = GlobalKey<NavigatorState>();
    await tester.pumpWidget(screen(session, navigator: navigator));
    await tester.pump();
    unawaited(
      navigator.currentState!.push(
        CupertinoPageRoute<void>(
          builder: (_) => const Scaffold(body: Text('Details')),
        ),
      ),
    );
    await tester.pumpAndSettle();
    pending.complete({
      'data': {
        'page': {'revision': 1, 'entries': <dynamic>[]},
        'missing': ['first'],
      },
    });
    await tester.pump();
    expect(reads, 1);

    navigator.currentState!.pop();
    await tester.pumpAndSettle();
    expect(reads, 2);
    expect(find.text('Latest message'), findsOneWidget);
    await release(tester, session);
  });

  testWidgets('returning to tasks applies the resource tab global selection', (
    tester,
  ) async {
    var firstReads = 0;
    var secondReads = 0;
    final first = HostConnection.test(
      id: 'first',
      label: 'First host',
      snapshot: {
        'sessions': [conversation(title: 'First host conversation')],
      },
      command: (kind, _) async {
        expect(kind, 'read_conversation');
        firstReads++;
        return preview('First preview');
      },
    );
    final second = HostConnection.test(
      id: 'second',
      label: 'Second host',
      snapshot: {
        'projects': [
          {'id': 'target', 'name': 'Target project'},
          {'id': 'other', 'name': 'Other project'},
        ],
        'worktrees': [
          {
            'id': 'target-tree',
            'project': 'target',
            'path': '/target',
            'main': true,
          },
          {
            'id': 'other-tree',
            'project': 'other',
            'path': '/other',
            'main': true,
          },
        ],
        'sessions': [
          {
            ...conversation(title: 'Selected project conversation'),
            'project': 'target',
          },
          {
            ...conversation(title: 'Other project conversation'),
            'id': 'other-session',
            'project': 'other',
          },
        ],
      },
      command: (kind, data) async {
        if (kind == 'read_conversation') {
          expect(data?['session'], 'session');
          secondReads++;
          return preview('Second preview');
        }
        expect(kind, 'inspect_git');
        return {
          'data': {'kind': 'directory'},
        };
      },
    );
    final session = AppSession.test(hosts: [first, second]);
    var tab = 0;
    await tester.pumpWidget(
      screen(
        session,
        home: StatefulBuilder(
          builder: (context, setState) => Scaffold(
            body: IndexedStack(
              index: tab,
              children: [
                TickerMode(enabled: tab == 0, child: const TasksPage()),
                TickerMode(enabled: tab == 1, child: const ResourcesPage()),
              ],
            ),
            bottomNavigationBar: Row(
              children: [
                TextButton(
                  onPressed: () => setState(() => tab = 0),
                  child: const Text('Tasks tab'),
                ),
                TextButton(
                  onPressed: () => setState(() => tab = 1),
                  child: const Text('Resources tab'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(firstReads, 1);
    expect(secondReads, 0);
    final previous = tester.widget<ConversationTask>(
      find.byType(ConversationTask),
    );

    await tester.tap(find.text('Resources tab'));
    await tester.pumpAndSettle();
    await tester.tap(
      find.byTooltip(
        tester.element(find.byType(ResourcesPage)).tr('selectHost'),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('host-row-second')));
    await tester.pumpAndSettle();
    await tester.tap(find.byType(ProjectBar));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(ListTile, 'Target project'));
    await tester.pumpAndSettle();
    expect(session.selectedHost, same(second));
    expect(session.selectedProjectId, 'target');
    expect(session.selectedWorktreeId, 'target-tree');
    expect(firstReads, 1);
    expect(secondReads, 0);
    expect(
      tester.widget<ConversationTask>(
        find.byType(ConversationTask, skipOffstage: false),
      ),
      same(previous),
    );

    await tester.tap(find.text('Tasks tab'));
    await tester.pumpAndSettle();
    expect(find.text('First host conversation'), findsNothing);
    expect(find.text('Other project conversation'), findsNothing);
    expect(find.text('Selected project conversation'), findsOneWidget);
    expect(find.text('Second preview'), findsOneWidget);
    expect(
      tester.widget<ConversationTask>(find.byType(ConversationTask)).host,
      same(second),
    );
    expect(firstReads, 1);
    expect(secondReads, 1);
    await release(tester, session);
  });

  testWidgets('conversation details ignore unrelated host notifications', (
    tester,
  ) async {
    final current = HostConnection.test(
      id: 'current',
      label: 'Current',
      connected: false,
      snapshot: {
        'sessions': [conversation()],
      },
    );
    final other = HostConnection.test(
      id: 'other',
      label: 'Other',
      connected: false,
    );
    final session = AppSession.test(hosts: [current, other]);
    await tester.pumpWidget(
      screen(
        session,
        home: LiveConversationPage(
          host: current,
          sessionId: 'session',
          initialSession: conversation(),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final previous = tester.widget<ConversationFrame>(
      find.byType(ConversationFrame),
    );
    other.notifyListeners();
    await tester.pump();
    expect(
      tester.widget<ConversationFrame>(find.byType(ConversationFrame)),
      same(previous),
    );
    session.selectHost(other.id);
    await tester.pump();
    expect(
      tester.widget<ConversationFrame>(find.byType(ConversationFrame)),
      same(previous),
    );
    expect(
      tester
          .widget<LiveConversationPage>(find.byType(LiveConversationPage))
          .host,
      same(current),
    );
    current.snapshot['sessions'] = [conversation(title: 'Own host update')];
    current.notifyListeners();
    await tester.pump();
    expect(
      tester.widget<ConversationFrame>(find.byType(ConversationFrame)).title,
      'Own host update',
    );
    final beforeClose = tester.widget<ConversationFrame>(
      find.byType(ConversationFrame),
    );
    await current.close();
    await tester.pump();
    expect(
      tester.widget<ConversationFrame>(find.byType(ConversationFrame)),
      isNot(same(beforeClose)),
    );
    await release(tester, session);
  });
}
