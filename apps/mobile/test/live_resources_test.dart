import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:webview_flutter_platform_interface/webview_flutter_platform_interface.dart';

import 'support/webview.dart';
import 'support/commands.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/commands.dart';
import 'package:sailry_bridge/api/ports.dart';
import 'package:sailry_mobile/features/resources/files_page.dart';
import 'package:sailry_mobile/features/resources/git_page.dart';
import 'package:sailry_mobile/features/resources/file_draft.dart';
import 'package:sailry_mobile/features/resources/live_resources.dart';
import 'package:sailry_mobile/features/resources/live_hosts.dart';
import 'package:sailry_mobile/features/resources/ports_page.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

const workspace = {
  'id': 'tree',
  'project': 'project',
  'path': '/projects/Actual project',
  'main': true,
};

class ForwardFixture implements Forwarding {
  final _values = StreamController<String>();
  late final _iterator = StreamIterator(_values.stream);
  bool closed = false;
  bool released = false;
  void stop() => _values.add(jsonEncode({'kind': 'closed'}));
  @override
  Future<int> localPort() async => 43123;
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
  void dispose() => released = true;
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class ConnectionFixture implements Connection {
  final requests = <String>[];
  final commands = CommandFixture();
  String? watchedSession;
  Map<String, dynamic>? source;
  @override
  Future<CommandUpdates> watchCommands({required String session}) async {
    watchedSession = session;
    return commands;
  }

  @override
  Future<Forwarding> forwardService({
    required String source,
    required int localPort,
  }) async {
    this.source = jsonDecode(source) as Map<String, dynamic>;
    return forwardPort(
      remotePort: (this.source!['service'] as Map)['port'] as int,
      localPort: localPort,
    );
  }

  final forward = ForwardFixture();
  (int, int)? forwarded;
  int forwardCalls = 0;
  Completer<Forwarding>? pendingForward;
  @override
  Future<String> execute({required String request}) async {
    requests.add(request);
    return jsonEncode({
      'Ok': {
        'kind': 'file_written',
        'data': {'revision': 'r2'},
      },
    });
  }

  @override
  Future<Forwarding> forwardPort({
    required int remotePort,
    required int localPort,
  }) async {
    forwardCalls++;
    forwarded = (remotePort, localPort);
    return pendingForward == null ? forward : await pendingForward!.future;
  }

  @override
  Future<void> close() async {}
  @override
  void dispose() {}
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Future<void> mount(WidgetTester tester, Widget page, AppSession session) async {
  tester.view.physicalSize = const Size(430, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    SessionScope(
      session: session,
      child: MaterialApp(theme: SailryTheme.of(Brightness.light), home: page),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('host changes clear workspace selection', (tester) async {
    final hosts = [
      for (final id in ['A', 'B'])
        HostConnection.test(
          id: id,
          label: 'Host $id',
          command: (_, _) async => {},
          snapshot: {
            'worktrees': [
              for (final suffix in ['main', 'branch'])
                {
                  'id': '$id-$suffix',
                  'project': 'project-$id',
                  'path': '/projects/$id-$suffix',
                },
            ],
          },
        ),
    ];
    final session = AppSession.test(hosts: hosts);
    await mount(tester, const LiveResourcesPage(), session);
    await tester.tap(find.text('A-main'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('A-branch'));
    await tester.pumpAndSettle();
    expect(find.text('A-branch'), findsOneWidget);
    session.selectHost('B');
    await tester.pumpAndSettle();
    expect(find.text('B-main'), findsOneWidget);
    expect(find.text('A-branch'), findsNothing);
    expect(find.text(tr('resourceNoWorkspace')), findsNothing);
    await tester.tap(find.byTooltip(tr('selectHost')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Host A'));
    await tester.pumpAndSettle();
    expect(find.text('A-main'), findsOneWidget);
    session.selectHost('B');
    await tester.pumpAndSettle();
    expect(find.text('B-main'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await session.close();
    session.dispose();
  });

  test('uncertain saves preserve the request and text', () async {
    final connection = ConnectionFixture();
    final commands = <Map<String, dynamic>>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (kind, data) async {
        commands.add(data!);
        throw const CommandFailure(
          'outcome_unknown',
          request: 'stable-save-request',
        );
      },
    );
    final draft = FileDraft({'text': 'original\r\n', 'revision': 'r1'});
    draft.text = '中文 😀\r\n';
    await expectLater(
      draft.save(host, 'tree', 'note.md'),
      throwsA(isA<CommandFailure>()),
    );
    expect(draft.dirty, isTrue);
    expect(draft.revision, 'r1');
    expect(
      () => draft.reload({'text': 'other', 'revision': 'r3'}),
      throwsStateError,
    );
    await draft.save(host, 'tree', 'note.md');
    expect(commands.single, {
      'worktree': 'tree',
      'path': 'note.md',
      'text': '中文 😀\r\n',
      'expected_revision': 'r1',
    });
    expect(connection.requests, ['stable-save-request']);
    expect(draft.text, '中文 😀\r\n');
    expect(draft.saved, draft.text);
    expect(draft.revision, 'r2');
    expect(draft.pending, isNull);
    expect(draft.dirty, isFalse);
  });

  test('partial previews cannot overwrite files', () async {
    var writes = 0;
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      command: (_, _) async {
        writes++;
        return {};
      },
    );
    final draft = FileDraft({
      'text': 'preview',
      'revision': 'r1',
      'truncated': true,
    });
    draft.text = 'replacement';
    await expectLater(draft.save(host, 'tree', 'large.txt'), throwsStateError);
    expect(writes, 0);
  });

  testWidgets('unpaired pages exclude preview projects', (tester) async {
    final session = AppSession.test();
    await mount(tester, const ResourcesPage(), session);
    expect(find.text(tr('hostConnectPrompt')), findsOneWidget);
    expect(find.text('sailry-web'), findsNothing);
    await tester.pumpWidget(const SizedBox());
    session.dispose();
  });

  for (final binding in ['local', 'remote']) {
    testWidgets('$binding conflicts retain drafts until reload', (
      tester,
    ) async {
      final commands = <(String, Map<String, dynamic>?)>[];
      final host = HostConnection.test(
        id: binding,
        label: binding,
        snapshot: {
          'worktrees': [workspace],
        },
        command: (kind, data) async {
          commands.add((kind, data));
          switch (kind) {
            case 'list_directory':
              return {
                'data': {
                  'entries': [
                    {'name': 'note.md', 'kind': 'file'},
                  ],
                },
              };
            case 'read_file':
              return {
                'data': {
                  'path': 'note.md',
                  'text': 'Original\r\n中文\r\n',
                  'revision': 'r1',
                },
              };
            case 'write_file':
              throw const CommandFailure('revision_conflict');
            default:
              throw StateError('Unexpected command: $kind');
          }
        },
      );
      final session = AppSession.test(hosts: [host]);
      await mount(
        tester,
        FilesPage(hostId: binding, worktreeId: 'tree'),
        session,
      );
      await tester.tap(find.text('note.md'));
      await tester.pumpAndSettle();
      await tester.tap(find.byTooltip(tr('edit')));
      await tester.pumpAndSettle();
      final field = find.byKey(const ValueKey('resource-file-input'));
      await tester.enterText(field, 'Changed **中文**\r\n');
      await tester.pump();
      await tester.ensureVisible(find.byTooltip(tr('save')));
      await tester.tap(find.byTooltip(tr('save')));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(field).controller!.text,
        'Changed **中文**\r\n',
      );
      expect(commands.where((entry) => entry.$1 == 'write_file').single.$2, {
        'worktree': 'tree',
        'path': 'note.md',
        'text': 'Changed **中文**\r\n',
        'expected_revision': 'r1',
      });
      expect(find.textContaining(tr('failureConflict')), findsOneWidget);
      await tester.ensureVisible(find.byTooltip(tr('refresh')));
      await tester.tap(find.byTooltip(tr('refresh')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('cancel')));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(field).controller!.text,
        'Changed **中文**\r\n',
      );
      expect(commands.where((entry) => entry.$1 == 'read_file'), hasLength(1));
      await tester.tap(find.byTooltip(tr('refresh')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('discard')));
      await tester.pumpAndSettle();
      expect(field, findsNothing);
      await tester.tap(find.byTooltip(tr('edit')));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(field).controller!.text,
        'Original\r\n中文\r\n',
      );
      expect(commands.where((entry) => entry.$1 == 'read_file'), hasLength(2));
      await tester.pumpWidget(const SizedBox());
      session.dispose();
    });

    testWidgets('$binding Git mutations use current revisions', (tester) async {
      var staged = false;
      var committed = false;
      final writes = <(String, Map<String, dynamic>)>[];
      final host = HostConnection.test(
        id: binding,
        label: binding,
        snapshot: {
          'worktrees': [workspace],
        },
        command: (kind, data) async {
          switch (kind) {
            case 'inspect_git':
              return {
                'data': {
                  'kind': 'ready',
                  'head': 'head',
                  'branch': 'main',
                  'index_revision': staged ? 'index2' : 'index1',
                  'entries': committed
                      ? []
                      : [
                          {
                            'path': 'actual.txt',
                            'staged': staged ? 'modified' : null,
                            'unstaged': staged ? null : 'modified',
                          },
                        ],
                },
              };
            case 'list_git_branches':
            case 'read_git_log':
              return {
                'data': {'entries': []},
              };
            case 'read_git_diff':
              return {
                'data': {'path': 'actual.txt', 'text': '-old\n+new\n'},
              };
            case 'update_git_index':
              writes.add((kind, data!));
              staged = true;
              return {'data': {}};
            case 'create_git_commit':
              writes.add((kind, data!));
              committed = true;
              return {
                'data': {'id': 'new-commit'},
              };
            default:
              throw StateError('Unexpected command: $kind');
          }
        },
      );
      final session = AppSession.test(hosts: [host]);
      await mount(
        tester,
        GitPage(hostId: binding, worktreeId: 'tree'),
        session,
      );
      await tester.tap(find.text('actual.txt'));
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('stage')));
      await tester.pumpAndSettle();
      expect(writes.first.$1, 'update_git_index');
      expect(writes.first.$2, {
        'worktree': 'tree',
        'paths': ['actual.txt'],
        'operation': 'stage',
        'expected_index': 'index1',
        'expected_head': 'head',
      });
      await tester.tap(find.text(tr('commit')));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextFormField), 'Save actual change');
      await tester.tap(find.text(tr('confirm')));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
      expect(writes.last.$1, 'create_git_commit');
      expect(writes.last.$2, containsPair('message', 'Save actual change'));
      expect(writes.last.$2, containsPair('expected_index', 'index2'));
      expect(writes.last.$2, containsPair('expected_head', 'head'));
      expect(writes.last.$2, containsPair('expected_branch', 'main'));
      expect(find.text(tr('noChanges')), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
      session.dispose();
    });
  }

  testWidgets('host mappings survive page exit and close with the host', (
    tester,
  ) async {
    final connection = ConnectionFixture();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (_, _) async => {},
    );
    final session = AppSession.test(hosts: [host]);
    await mount(tester, const LiveHostsPage(), session);
    await tester.tap(find.byTooltip(tr('ports')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), '3000');
    await tester.tap(find.text(tr('resourceOpenPort')));
    await tester.pumpAndSettle();
    expect(connection.forwarded, (3000, 0));
    expect(find.text('http://127.0.0.1:43123'), findsOneWidget);
    expect(connection.forward.closed, isFalse);
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(connection.forward.closed, isFalse);
    await mount(tester, PortsPage(host: host), session);
    expect(find.text('3000 → 43123'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await host.close();
    expect(connection.forward.closed, isTrue);
    expect(connection.forward.released, isTrue);
    session.dispose();
  });

  testWidgets('preview reuses the tunnel and reports disconnection', (
    tester,
  ) async {
    final browser = BrowserPlatform();
    WebViewPlatform.instance = browser;
    final connection = ConnectionFixture();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (_, _) async => {},
    );
    final session = AppSession.test(hosts: [host]);
    await mount(tester, PortsPage(host: host), session);
    await tester.enterText(find.byType(TextField), '3000');
    await tester.tap(find.text(tr('resourceOpenPort')));
    await tester.pumpAndSettle();
    for (var index = 0; index < 2; index++) {
      await tester.tap(find.text(tr('resourceOpenBrowser')));
      await tester.tap(
        find.text(tr('resourceOpenBrowser')),
        warnIfMissed: false,
      );
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 400));
      browser.navigation.finished('http://127.0.0.1:43123');
      await tester.pumpAndSettle();
      expect(browser.controller.requests, [
        Uri.parse('http://127.0.0.1:43123'),
      ]);
      expect(connection.forwardCalls, 1);
      if (index == 1) {
        connection.forward.stop();
        await tester.pumpAndSettle();
        expect(find.text(tr('resourceForwardStopped')), findsOneWidget);
      }
      await tester.tap(find.byTooltip(tr('back')));
      await tester.pumpAndSettle();
      expect(connection.forward.closed, isFalse);
    }
    await tester.tap(find.text(tr('closePort')));
    await tester.pumpAndSettle();
    expect(find.text('http://127.0.0.1:43123'), findsNothing);
    expect(connection.forward.closed, isTrue);
    expect(connection.forward.released, isTrue);
    await tester.pumpWidget(const SizedBox());
    session.dispose();
  });

  testWidgets('pending forwarding is deduplicated and released with its host', (
    tester,
  ) async {
    final connection = ConnectionFixture()
      ..pendingForward = Completer<Forwarding>();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (_, _) async => {},
    );
    final session = AppSession.test(hosts: [host]);
    await mount(tester, PortsPage(host: host), session);
    await tester.enterText(find.byType(TextField), '0');
    await tester.tap(find.text(tr('resourceOpenPort')));
    await tester.pumpAndSettle();
    expect(find.text(tr('resourceInvalidPort')), findsOneWidget);
    expect(connection.forwardCalls, 0);
    await tester.enterText(find.byType(TextField), '3000');
    await tester.tap(find.text(tr('resourceOpenPort')));
    await tester.tap(find.text(tr('resourceOpenPort')));
    await tester.pump();
    expect(connection.forwardCalls, 1);
    await tester.pumpWidget(const SizedBox());
    final closing = host.close();
    connection.pendingForward!.complete(connection.forward);
    await closing;
    await tester.pump();
    expect(connection.forward.closed, isTrue);
    expect(connection.forward.released, isTrue);
    expect(tester.takeException(), isNull);
    session.dispose();
  });

  testWidgets('session services map on click and preserve their URL', (
    tester,
  ) async {
    final browser = BrowserPlatform();
    WebViewPlatform.instance = browser;
    final connection = ConnectionFixture();
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      connection: connection,
      command: (_, _) async => {},
    );
    final session = AppSession.test(hosts: [host]);
    connection.commands.emit({
      'connected': true,
      'items': [
        {
          'id': 'command-a',
          'session': 'session-a',
          'services': [
            {'port': 3000, 'url': 'http://127.0.0.1:3000/app?q=1'},
          ],
        },
      ],
    });
    await mount(tester, PortsPage(host: host, session: 'session-a'), session);
    expect(connection.watchedSession, 'session-a');
    expect(find.byKey(const ValueKey('service-3000')), findsOneWidget);
    expect(connection.forwardCalls, 0);
    await tester.tap(find.byTooltip(tr('resourceServiceOpen')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));
    browser.navigation.finished('http://127.0.0.1:43123/app?q=1');
    await tester.pumpAndSettle();
    expect(browser.controller.requests, [
      Uri.parse('http://127.0.0.1:43123/app?q=1'),
    ]);
    expect(connection.source?['session'], 'session-a');
    expect(connection.source?['command'], 'command-a');
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    expect(find.text('3000 → 43123'), findsNWidgets(2));
    expect(connection.forwardCalls, 1);
    connection.commands.emit({'connected': true, 'items': []});
    connection.forward.stop();
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('service-3000')), findsNothing);
    expect(find.text(tr('resourceForwardStopped')), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await host.close();
    expect(connection.commands.closed, isTrue);
    session.dispose();
  });
}
