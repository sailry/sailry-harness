import 'dart:async';
import 'dart:convert';

import 'package:fake_async/fake_async.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/demo/connection.dart';
import 'package:sailry_mobile/features/conversations/live/modes.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/conversations/live/task.dart';
import 'package:sailry_mobile/features/resources/pair_form.dart';
import 'package:sailry_mobile/features/resources/git/model.dart' as git;
import 'package:sailry_mobile/features/terminal/emulator.dart';
import 'package:sailry_mobile/features/terminal/grid.dart';
import 'package:sailry_mobile/features/terminal/session.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/runtime/speech.dart';

import 'support/updates.dart' as update_fixture;

class _Preferences extends Fake implements SharedPreferencesAsync {
  final values = <String, bool>{};
  @override
  Future<bool?> getBool(String key) async => values[key];
  @override
  Future<void> setBool(String key, bool value) async {
    values[key] = value;
  }
}

class _Speech extends SpeechController {
  _Speech(super.directory) : super(preferences: _Preferences());
  @override
  Future<void> check() async {}
}

class _GenuineConnection extends Fake implements Connection {
  _GenuineConnection(this.fixture);
  final DemoConnection fixture;
  @override
  Future<Updates> watch() => fixture.watch();
  @override
  Future<String> prepare({required String command}) =>
      fixture.prepare(command: command);
  @override
  Future<String> execute({required String request}) =>
      fixture.execute(request: request);
  @override
  Future<void> close() => fixture.close();
  @override
  void dispose() => fixture.dispose();
}

class _Owner extends Fake implements Controller {
  _Owner({Translator translate = tr})
    : channel = DemoConnection(translate: translate);
  final addresses = <String>[];
  final forgotten = <String>[];
  final DemoConnection channel;
  Future<List<String>>? pendingPeers;
  final peersStarted = Completer<void>();
  String? pin;
  @override
  Future<List<String>> savedPeers() async => addresses;
  @override
  Future<List<String>> peers() async {
    if (!peersStarted.isCompleted) peersStarted.complete();
    return pendingPeers ?? addresses;
  }

  @override
  Future<Connection> connect({required String address}) async =>
      _GenuineConnection(channel);
  @override
  Future<String> pairPin({required String origin, required String pin}) async {
    this.pin = pin;
    return jsonEncode({'id': 'genuine-host'});
  }

  @override
  Future<void> forget({required String address}) async {
    forgotten.add(address);
  }

  @override
  Future<void> close() async {}
  @override
  void dispose() {}
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test(
    'fresh offline startup and reserved PIN never open native transport',
    () async {
      final preferences = _Preferences();
      final session = AppSession.test(
        ready: false,
        preferences: preferences,
        savedPeers: (_) async => false,
        initialize: () async => fail('Demo initialized native transport'),
        openController: (_, _) async =>
            throw StateError('Demo opened a controller'),
      );
      await session.start(path: '/isolated/controller');
      expect(session.ready, isTrue);
      expect(session.error, isNull);
      expect(session.controller, isNull);
      final host = await session.pairPin(' 888888 ');
      expect(host.isDemo, isTrue);
      expect(host.connected, isTrue);
      expect(host.address, isEmpty);
      expect(await session.pairPin('888888'), same(host));
      expect(session.hosts, [host]);
      expect(session.controller, isNull);
      expect(jsonEncode(host.snapshot), isNot(contains('credential')));
      expect(preferences.values['hosts.demo.enabled'], isTrue);
      expect(preferences.values['hosts.demo.selected'], isTrue);
      await session.close();
      session.dispose();
    },
  );

  test(
    'saved demo reopens offline and removes only its local preference',
    () async {
      final preferences = _Preferences()
        ..values.addAll({
          'hosts.demo.enabled': true,
          'hosts.demo.selected': true,
        });
      var peerReads = 0;
      final session = AppSession.test(
        ready: false,
        preferences: preferences,
        savedPeers: (_) async {
          peerReads++;
          return false;
        },
        initialize: () async => fail('Saved demo initialized native transport'),
      );
      await session.start(path: '/isolated/controller');
      expect(peerReads, 0);
      final host = session.selectedHost!;
      expect(host.isDemo, isTrue);
      final removing = session.removeHost(host);
      await Future.wait([removing, session.removeHost(host)]);
      expect(session.hosts, isEmpty);
      expect(preferences.values['hosts.demo.enabled'], isFalse);
      expect(preferences.values['hosts.demo.selected'], isFalse);
      expect(session.controller, isNull);
      expect(peerReads, 1);
      await session.close();
      session.dispose();
    },
  );

  test(
    'removing a restored demo reconnects existing trust without re-pairing',
    () async {
      final preferences = _Preferences()
        ..values.addAll({
          'hosts.demo.enabled': true,
          'hosts.demo.selected': true,
        });
      final directory = Completer<List<String>>();
      final owner = _Owner()
        ..addresses.add(jsonEncode({'id': 'trusted-host'}))
        ..pendingPeers = directory.future;
      var initialized = 0;
      final session = AppSession.test(
        ready: false,
        preferences: preferences,
        savedPeers: (_) async => true,
        initialize: () async {
          initialized++;
        },
        openController: (_, _) async => owner,
        createSpeech: _Speech.new,
      );
      await session.start(path: '/isolated/controller');
      expect(initialized, 0);
      await session.removeHost(session.selectedHost!);
      expect(initialized, 1);
      expect(session.selectedHost!.id, 'trusted-host');
      expect(session.selectedHost!.isDemo, isFalse);
      expect(owner.pin, isNull);
      expect(owner.forgotten, isEmpty);
      expect(owner.addresses, hasLength(1));
      await owner.peersStarted.future;
      expect(directory.isCompleted, isFalse);
      directory.complete(owner.addresses);
      await session.refreshHosts();
      expect(session.selectedHost!.id, 'trusted-host');
      await session.close();
      session.dispose();
      await owner.channel.close();
    },
  );

  test('ordinary PIN pairing still opens the real controller', () async {
    final owner = _Owner();
    var initialized = 0;
    final session = AppSession.test(
      ready: false,
      savedPeers: (_) async => false,
      initialize: () async {
        initialized++;
      },
      openController: (_, _) async => owner,
      createSpeech: _Speech.new,
    );
    await session.start(path: '/isolated/controller');
    expect(initialized, 0);
    final host = await session.pairPin(' 123456 ');
    expect(host.id, 'genuine-host');
    expect(host.isDemo, isFalse);
    expect(owner.pin, '123456');
    expect(initialized, 1);
    await session.close();
    session.dispose();
    await owner.channel.close();
  });

  testWidgets('pairing a real host replaces the visible demo catalog', (
    tester,
  ) async {
    final owner = _Owner(
      translate: (key) =>
          key == 'demoConversationTitle' ? 'Paired host conversation' : tr(key),
    );
    // The non-demo adapter starts with an empty Node snapshot.
    (owner.channel.snapshot['sessions'] as List).clear();
    final preferences = _Preferences();
    final session = AppSession.test(
      controller: owner,
      preferences: preferences,
    );
    final demo = await session.pairPin('888888');
    await tester.pumpWidget(SailryApp(session: session));
    await tester.pump();
    expect(find.text(tr('demoConversationTitle')), findsOneWidget);

    final live = await session.pairPin('123456');
    await tester.pump();
    expect(session.selectedHost, same(live));
    expect(session.hosts, [demo, live]);
    expect(session.resourceHosts, [live]);
    expect(preferences.values['hosts.demo.enabled'], isTrue);
    expect(preferences.values['hosts.demo.selected'], isFalse);
    expect(find.byType(ConversationTask), findsNothing);

    final created = await live.command('create_session', {
      'config': <String, dynamic>{},
    });
    await tester.pump();
    expect(find.byType(ConversationTask), findsOneWidget);
    expect(find.text('Paired host conversation'), findsOneWidget);
    expect(
      tester
          .widget<ConversationTask>(find.byType(ConversationTask))
          .session['id'],
      object(created['data'])['id'],
    );

    session.selectHost(demo.id);
    await tester.pump();
    expect(session.resourceHosts, [demo]);
    expect(find.text(tr('demoConversationTitle')), findsOneWidget);
    expect(find.text('Paired host conversation'), findsNothing);
    expect(find.byType(ConversationTask), findsNWidgets(2));
    session.selectHost(live.id);
    await tester.pump();
    expect(find.text(tr('demoConversationTitle')), findsNothing);
    expect(find.text('Paired host conversation'), findsOneWidget);
    expect(find.byType(ConversationTask), findsOneWidget);
    expect(tester.takeException(), isNull);

    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
    await owner.channel.close();
  });

  test(
    'demo replies stream, stop works, and file edits reset on reopening',
    () {
      fakeAsync((clock) {
        final session = AppSession.test(savedPeers: (_) async => false);
        unawaited(session.pairPin('888888'));
        clock.flushMicrotasks();
        final host = session.selectedHost!;
        final modes = ConversationModes(host);
        expect(modes.selected({'mode': 'code'}), 'code');
        expect(modes.choices({'mode': 'code'}), isEmpty);
        expect(modes.select({}, 'plan')!['mode'], 'plan');
        modes.dispose();
        Map<String, dynamic>? run;
        unawaited(
          host
              .command('submit_turn', {
                'session': 'demo-login',
                'expected_revision': 3,
                'message': {'text': 'Test a local reply', 'attachments': []},
              })
              .then((result) => run = object(result['data'])),
        );
        clock.flushMicrotasks();
        clock.elapse(const Duration(milliseconds: 90));
        Map<String, dynamic>? history;
        unawaited(
          host
              .command('read_conversation', {'session': 'demo-login'})
              .then((result) => history = object(result['data'])),
        );
        clock.flushMicrotasks();
        final answer = objects(object(history!['page'])['entries']).last;
        expect(text(objects(answer['parts']).single['data']), isNotEmpty);
        unawaited(host.command('stop_turn', {'turn': run!['turn']}));
        clock.flushMicrotasks();
        clock.elapse(const Duration(seconds: 5));
        unawaited(
          host
              .command('read_conversation', {'session': 'demo-login'})
              .then((result) => history = object(result['data'])),
        );
        clock.flushMicrotasks();
        expect(
          objects(object(history!['page'])['runs']).last['status'],
          'stopped',
        );
        Map<String, dynamic>? file;
        unawaited(
          host
              .command('read_file', {'path': 'README.md'})
              .then((result) => file = object(result['data'])),
        );
        clock.flushMicrotasks();
        final original = file!['text'];
        unawaited(
          host.command('write_file', {
            'path': 'README.md',
            'text': 'Local edit',
            'expected_revision': file!['revision'],
          }),
        );
        clock.flushMicrotasks();
        unawaited(
          host
              .command('read_file', {'path': 'README.md'})
              .then((result) => file = object(result['data'])),
        );
        clock.flushMicrotasks();
        expect(file!['text'], 'Local edit');
        final reopened = DemoConnection(translate: tr);
        String? request;
        unawaited(
          reopened
              .prepare(
                command: jsonEncode({
                  'kind': 'read_file',
                  'data': {'path': 'README.md'},
                }),
              )
              .then((value) => request = value),
        );
        clock.flushMicrotasks();
        Map<String, dynamic>? result;
        unawaited(
          reopened
              .execute(request: request!)
              .then((value) => result = object(jsonDecode(value))),
        );
        clock.flushMicrotasks();
        expect(object(object(result!['Ok'])['data'])['text'], original);
        unawaited(reopened.close());
        unawaited(session.close());
        clock.flushMicrotasks();
        session.dispose();
      });
    },
  );

  test(
    'demo workspace changes and terminal input use local projections',
    () async {
      final session = AppSession.test(
        initialize: () async => fail('Demo initialized native transport'),
      );
      final host = await session.pairPin('888888');
      final listing = await host.command('list_directory', {'path': ''});
      expect(
        objects(
          object(listing['data'])['entries'],
        ).map((entry) => entry['name']),
        containsAll(['README.md', 'src', 'docs']),
      );
      final diff = await host.command('read_git_diff', {
        'worktree': 'demo-lumen-main',
        'path': 'src/styles/login.css',
        'scope': 'unstaged',
      });
      expect(object(diff['data'])['text'], contains('+  gap: 20px;'));
      await host.command('update_git_index', {
        'worktree': 'demo-lumen-main',
        'operation': 'stage',
        'paths': ['src/styles/login.css'],
      });
      final repository = await host.command('inspect_git', {
        'worktree': 'demo-lumen-main',
      });
      final groups = git.groupChanges(
        objects(object(repository['data'])['entries']),
      );
      expect(groups.unstaged, isEmpty);
      expect(groups.staged.single['path'], 'src/styles/login.css');

      final terminal = TerminalSession(
        controller: await session.controllerIdentity(host),
        command: host.command,
        resolve: host.check,
        worktree: 'demo-lumen-main',
      );
      final opened = await terminal.open({
        'viewport': {'columns': 80, 'rows': 24},
      }, objects(host.snapshot['terminals']));
      expect(terminal.lease(opened), isNotNull);
      final updates = await host.connection.watchTerminal(
        terminal: terminal.terminal!,
      );
      await updates.next();
      await host.command('input_terminal', {
        'terminal': terminal.terminal,
        'revision': terminal.lease(opened),
        'inputs': [
          {
            'text': {'text': 'echo '},
          },
          {
            'paste': {'text': 'offline'},
          },
          {
            'key': {
              'event': {'key': 'enter'},
            },
          },
        ],
      });
      final view = object(jsonDecode(await updates.next()));
      final source = object(object(view['snapshot'])['screen']);
      final emulator = TerminalEmulator()
        ..feed(object(source['vt']), reset: view['reset'] == true);
      final rows = terminalLines(emulator.screen(source, {}));
      final visible = rows
          .map(
            (row) => terminalSelection([row], (row: 0, column: 0), (
              row: 0,
              column: 499,
            )),
          )
          .join('\n');
      expect(visible, contains('echo offline'));
      expect(visible, contains('offline'));
      expect(session.controller, isNull);
      await updates.close();
      updates.dispose();
      await session.close();
      session.dispose();
    },
  );

  testWidgets('local host uses normal presentation without update requests', (
    tester,
  ) async {
    var checks = 0;
    final updates = update_fixture.updates(
      request: (_) async {
        checks++;
        throw StateError('Demo made an update request');
      },
    );
    final session = AppSession.test(
      ready: false,
      savedPeers: (_) async => false,
      initialize: () async => fail('Demo initialized native transport'),
    );
    await tester.pumpWidget(SailryApp(session: session, updates: updates));
    await session.start(path: '/isolated/controller');
    await tester.pumpAndSettle();
    expect(checks, 0);
    await tester.tap(find.byKey(const ValueKey('tab-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('pair')));
    // The PIN field has a repeating cursor animation.
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    expect(find.byType(PairForm), findsOneWidget);
    await tester.enterText(
      find.descendant(
        of: find.byType(PairForm),
        matching: find.byType(EditableText),
      ),
      '888888',
    );
    await tester.tap(find.widgetWithText(FilledButton, tr('pairAction')));
    // The running sample terminal keeps its status pulse active.
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    expect(session.selectedHost?.isDemo, isTrue);
    expect(find.byType(PairForm), findsNothing);
    expect(find.byKey(const ValueKey('demo-notice')), findsNothing);
    expect(find.text('MacBook Pro'), findsOneWidget);
    expect(find.text('macOS 26.1'), findsWidgets);
    expect(find.textContaining('demonstration'), findsNothing);
    await tester.tap(find.byKey(const ValueKey('tab-0')));
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    await tester.tap(find.text(tr('demoConversationTitle')).first);
    await tester.pumpAndSettle();
    expect(find.byType(LiveConversationPage), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.tap(find.byKey(const ValueKey('conversation-settings')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('configuration-mode')), findsOneWidget);
    expect(find.byKey(const ValueKey('demo-notice')), findsNothing);
    expect(tester.takeException(), isNull);
    expect(checks, 0);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
    updates.dispose();
  });
}
