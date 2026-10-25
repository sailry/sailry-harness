import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/creation_draft.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'support/drafts.dart';

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
      command: (kind, data) async {
        expect(kind, 'create_session');
        requested = data;
        return {
          'data': {...admitted(data!), 'worktree': 'node-workspace'},
        };
      },
    );
    final draft = CreationDraft(host);
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    expect(requested, isNull);
    expect(draft.worktree, isNull);
    await draft.send(hasInput: true);
    expect(requested!['project'], isNull);
    expect(requested!['worktree'], isNull);
    expect(draft.created!['worktree'], 'node-workspace');
  });

  test('late resources capture defaults once', () async {
    Map<String, dynamic>? requested;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: {},
      command: (_, data) async {
        requested = data;
        return {'data': admitted(data!)};
      },
    );
    final draft = CreationDraft(host);
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    expect(draft.hasModel, isFalse);
    await draft.send(hasInput: true);
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
    await draft.send(hasInput: true);
    expect(requested, {
      'project': 'project-a',
      'worktree': 'tree-a',
      'config': {...config, 'options': <String, dynamic>{}},
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
      command: (_, data) async {
        requests.add(data!);
        return {'data': admitted(data)};
      },
    );
    final draft = CreationDraft(host);
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
    await draft.send(hasInput: false);
    expect(requests, isEmpty);
    await draft.send(hasInput: true);
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
        command: (_, data) {
          requested = data;
          return reply.future;
        },
      );
      final app = AppSession.test(hosts: [first, second]);
      app.selectHost(second.id);
      final draft = CreationDraft(app.selectedHost!);
      addTearDown(draft.dispose);
      addTearDown(app.dispose);
      final sending = draft.send(hasInput: true);
      expect(draft.busy, isTrue);
      app.selectHost(first.id);
      second.snapshot = snapshot('b', config: {...config, 'model': 'b-first'});
      second.notifyListeners();
      expect(draft.config['model'], 'b-second');
      expect(requested, {
        'project': 'project-b',
        'worktree': 'tree-b',
        'config': {...config, 'options': <String, dynamic>{}},
      });
      reply.complete({'data': admitted(requested!)});
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
    final draft = CreationDraft(host);
    addTearDown(draft.dispose);
    addTearDown(host.dispose);
    expect(draft.hasModel, isFalse);
    await draft.send(hasInput: true);
    host.snapshot = snapshot('a')..['worktrees'] = [];
    host.notifyListeners();
    expect(draft.hasModel, isTrue);
    expect(draft.worktree, isNull);
    await draft.send(hasInput: true);
    expect(draft.created, isNull);
  });

  test('late admission after disposal leaves no controller updates', () async {
    final reply = Completer<Map<String, dynamic>>();
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      snapshot: snapshot('a'),
      command: (_, _) => reply.future,
    );
    addTearDown(host.dispose);
    final draft = CreationDraft(host);
    final sending = draft.send(hasInput: true);
    draft.dispose();
    reply.complete({'data': {}});
    await sending;
    expect(draft.created, isNull);
  });
}
