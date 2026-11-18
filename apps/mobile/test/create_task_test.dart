import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/creation_draft.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'support/drafts.dart';

CommandHandler startAndRead(
  void Function(Map<String, dynamic>) onStart, {
  String? worktree,
}) {
  Map<String, dynamic>? session;
  return (kind, data) async {
    if (kind == 'read_session') {
      expect(data, {'session': 'created'});
      return sessionReceipt(session!);
    }
    expect(kind, 'start_session');
    onStart(data!);
    session = {...admitted(data), 'worktree': ?worktree};
    return startReceipt();
  };
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('unassigned creation uses the Node workspace', () async {
    Map<String, dynamic>? requested;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a')
        ..['projects'] = []
        ..['worktrees'] = [
          {'id': 'another-session', 'project': null},
        ],
      command: startAndRead(
        (data) => requested = data,
        worktree: 'node-workspace',
      ),
    );
    final draft = CreationDraft(host);
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    expect(requested, isNull);
    expect(draft.worktree, isNull);
    await draft.send(message: 'First message');
    expect(requested!['project'], isNull);
    expect(requested!['worktree'], isNull);
    expect(draft.created!['worktree'], 'node-workspace');
  });

  test('project scope survives resource updates', () async {
    for (final (initial, project, worktree) in <(String?, String?, String?)>[
      (null, null, null),
      ('project-a', 'project-a', 'tree-a'),
      ('', null, null),
    ]) {
      Map<String, dynamic>? requested;
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        snapshot: snapshot('a'),
        command: startAndRead((data) => requested = data),
      );
      final draft = CreationDraft(host, initialProject: initial);
      addTearDown(draft.dispose);
      addTearDown(host.dispose);
      expect(draft.project?['id'], project);
      expect(draft.worktree?['id'], worktree);
      host.snapshot = snapshot('a')
        ..['projects'] = [
          {'id': 'project-b'},
          {'id': 'project-a'},
        ]
        ..['worktrees'] = [
          {'id': 'tree-b', 'project': 'project-b'},
          {'id': 'tree-a', 'project': 'project-a'},
        ];
      host.notifyListeners();
      expect(draft.project?['id'], project);
      expect(draft.worktree?['id'], worktree);
      await draft.send(message: 'First message');
      expect(requested!['project'], project);
      expect(requested!['worktree'], worktree);
    }
  });

  test('explicit project overrides the chats default', () async {
    Map<String, dynamic>? requested;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a'),
      command: startAndRead((data) => requested = data),
    );
    final draft = CreationDraft(host, initialProject: '');
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    draft.project = {'id': 'project-a'};
    draft.worktree = {'id': 'tree-a', 'project': 'project-a'};
    host.snapshot = snapshot('a');
    host.notifyListeners();
    expect(draft.project?['id'], 'project-a');
    expect(draft.worktree?['id'], 'tree-a');
    await draft.send(message: 'First message');
    expect(requested!['project'], 'project-a');
    expect(requested!['worktree'], 'tree-a');
  });

  test('creation retains the selected workspace within its project', () async {
    Map<String, dynamic>? requested;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a')
        ..['worktrees'] = [
          {'id': 'tree-a', 'project': 'project-a'},
          {'id': 'tree-linked', 'project': 'project-a'},
          {'id': 'tree-foreign', 'project': 'project-b'},
        ],
      command: startAndRead((data) => requested = data),
    );
    final draft = CreationDraft(
      host,
      initialProject: 'project-a',
      initialWorktree: 'tree-linked',
    );
    final foreign = CreationDraft(
      host,
      initialProject: 'project-a',
      initialWorktree: 'tree-foreign',
    );
    final unassigned = CreationDraft(host, initialWorktree: 'tree-linked');
    addTearDown(host.dispose);
    addTearDown(draft.dispose);
    addTearDown(foreign.dispose);
    addTearDown(unassigned.dispose);
    expect(draft.worktree?['id'], 'tree-linked');
    expect(foreign.worktree?['id'], 'tree-a');
    expect(unassigned.worktree, isNull);
    host.notifyListeners();
    expect(draft.worktree?['id'], 'tree-linked');
    await draft.send(message: 'First message');
    expect(requested!['project'], 'project-a');
    expect(requested!['worktree'], 'tree-linked');
  });

  test('Chats selection survives host refresh and clears worktree', () {
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a'),
    );
    final draft = CreationDraft(host, initialProject: 'project-a');
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    expect(draft.worktree?['id'], 'tree-a');
    draft.project = null;
    expect(draft.project, isNull);
    expect(draft.worktree, isNull);
    host.snapshot = snapshot('b');
    host.notifyListeners();
    expect(draft.project, isNull);
    expect(draft.worktree, isNull);
    draft.project = {'id': 'project-b'};
    expect(draft.worktree?['id'], 'tree-b');
  });

  test('late resources capture defaults once', () async {
    Map<String, dynamic>? requested;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {},
      command: startAndRead((data) => requested = data),
    );
    final draft = CreationDraft(host, initialProject: 'project-a');
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    expect(draft.hasModel, isFalse);
    await draft.send(message: 'First message');
    expect(requested, isNull);
    final config = {
      'mode': 'plan',
      'permission': 'deny',
      'provider': 'a',
      'model': 'a-second',
      'effort': 'high',
      'credential': provider('a')['credential'],
    };
    host.snapshot = snapshot('a', config: config);
    host.notifyListeners();
    expect(draft.hasModel, isTrue);
    host.snapshot = snapshot('a', config: {...config, 'model': 'a-first'});
    host.notifyListeners();
    await draft.send(message: 'First message');
    expect(requested, {
      'project': 'project-a',
      'worktree': 'tree-a',
      'config': {...config, 'options': <String, dynamic>{}},
      'message': {'text': 'First message', 'attachments': <String>[]},
      'execution': 'team',
      'team': null,
      'models': <String, dynamic>{},
    });
  });

  test('workspace selection changes only the new session', () async {
    final requests = <Map<String, dynamic>>[];
    final data = snapshot('a');
    final next = {'id': 'next-project'};
    final tree = {'id': 'next-feature', 'project': 'next-project'};
    (data['projects'] as List).add(next);
    (data['worktrees'] as List<Map<String, dynamic>>).addAll([
      {'id': 'next-main', 'project': 'next-project'},
      tree,
    ]);
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: data,
      command: startAndRead(requests.add),
    );
    final draft = CreationDraft(host, initialProject: 'project-a');
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    draft.project = next;
    expect(draft.worktrees.map((tree) => tree['id']), [
      'next-main',
      'next-feature',
    ]);
    draft.worktree = tree;
    draft.configure({
      ...draft.config,
      'model': 'a-second',
      'permission': 'project',
    });
    expect(requests, isEmpty);
    await draft.send(message: '');
    expect(requests, isEmpty);
    await draft.send(message: 'First message');
    expect(requests.single['project'], 'next-project');
    expect(requests.single['worktree'], 'next-feature');
    expect((requests.single['config'] as Map)['model'], 'a-second');
    expect((host.snapshot['defaults'] as Map)['config'], isNull);
  });

  test(
    'pending admission keeps its execution host and configuration',
    () async {
      final reply = Completer<Map<String, dynamic>>();
      Map<String, dynamic>? requested;
      final config = {
        'provider': 'b',
        'model': 'b-second',
        'effort': {'budget': 2048},
        'mode': 'plan',
        'permission': 'full',
        'credential': provider('b')['credential'],
      };
      final first = HostConnection.test(
        id: 'first',
        label: 'First',
        command: (_, _) async => throw StateError('Wrong execution host'),
      );
      final second = HostConnection.test(
        id: 'second',
        label: 'Second',
        snapshot: snapshot('b', config: config),
        command: (kind, data) async {
          if (kind == 'read_session') {
            return sessionReceipt(admitted(requested!));
          }
          expect(kind, 'start_session');
          requested = data;
          return reply.future;
        },
      );
      final app = AppSession.test(hosts: [first, second]);
      app.selectHost(second.id);
      final draft = CreationDraft(
        app.selectedHost!,
        initialProject: 'project-b',
      );
      addTearDown(draft.dispose);
      addTearDown(app.dispose);
      final sending = draft.send(
        message: 'First message',
        attachments: ['file'],
      );
      expect(draft.busy, isTrue);
      app.selectHost(first.id);
      second.snapshot = snapshot('b', config: {...config, 'model': 'b-first'});
      second.notifyListeners();
      expect(draft.config['model'], 'b-second');
      expect(requested, {
        'project': 'project-b',
        'worktree': 'tree-b',
        'config': {...config, 'options': <String, dynamic>{}},
        'message': {
          'text': 'First message',
          'attachments': ['file'],
        },
        'execution': 'team',
        'team': null,
        'models': <String, dynamic>{},
      });
      reply.complete(startReceipt());
      await sending;
      expect(draft.host, same(second));
      expect(draft.created!['config'], requested!['config']);
      await app.close();
    },
  );

  test('missing models and workspace prevent creation', () async {
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot(
        'a',
        providers: [
          provider('a', enabled: false),
          {...provider('b'), 'models': []},
        ],
      ),
      command: (_, _) async =>
          throw StateError('Unavailable draft was admitted'),
    );
    final draft = CreationDraft(host, initialProject: 'project-a');
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    expect(draft.hasModel, isFalse);
    await draft.send(message: 'First message');
    host.snapshot = snapshot('a')..['worktrees'] = [];
    host.notifyListeners();
    expect(draft.hasModel, isTrue);
    expect(draft.worktree, isNull);
    await draft.send(message: 'First message');
    expect(draft.created, isNull);
  });

  test('execution offers only credentials owned by the selected Node', () {
    final target = List.filled(32, '02').join();
    final entry = List.filled(32, '01').join();
    final foreign = {
      ...provider('a'),
      'credential': {'id': 'foreign', 'node': List.filled(32, 1)},
    };
    final local = {
      ...provider('b'),
      'credential': {'id': 'local', 'node': List.filled(32, 2)},
    };
    final anonymous = {...provider('c'), 'credential': null};
    final host = HostConnection.test(
      id: '$entry/$target',
      label: 'Host',
      snapshot: snapshot(
        'a',
        config: {
          'provider': 'a',
          'model': 'a-first',
          'credential': foreign['credential'],
          'features': {'speed': 'fast'},
        },
        providers: [foreign, local, anonymous],
      ),
    );
    final draft = CreationDraft(host);
    addTearDown(host.dispose);
    addTearDown(draft.dispose);
    expect(draft.providers, [local, anonymous]);
    expect(draft.config['provider'], 'b');
    expect(draft.config['features'], isNull);
    expect(draft.hasModel, isTrue);
    expect(host.snapshot['providers'], [foreign, local, anonymous]);
  });

  test(
    'attachment-only first input is admitted with its captured tree',
    () async {
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        snapshot: snapshot('a'),
        command: startAndRead((data) {
          expect(data['worktree'], 'tree-a');
          expect(data['message'], {
            'text': '',
            'attachments': ['image'],
          });
        }),
      );
      final draft = CreationDraft(host, initialProject: 'project-a');
      addTearDown(host.dispose);
      addTearDown(draft.dispose);
      await draft.send(message: '', attachments: ['image']);
      expect(draft.created?['id'], 'created');
    },
  );

  test('late admission after disposal leaves no controller updates', () async {
    final reply = Completer<Map<String, dynamic>>();
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a'),
      command: (kind, data) {
        expect(kind, 'start_session');
        expect(data?['message'], {'text': 'First message', 'attachments': []});
        return reply.future;
      },
    );
    addTearDown(host.dispose);
    final draft = CreationDraft(host, initialProject: 'project-a');
    final sending = draft.send(message: 'First message');
    draft.dispose();
    reply.complete(startReceipt());
    await sending;
    expect(draft.created, isNull);
  });
}
