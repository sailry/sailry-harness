import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/draft_branches.dart';
import 'package:sailry_mobile/runtime/session.dart';

const primary = {'id': 'main', 'project': 'project', 'path': '/repo'};
const feature = {
  'id': 'feature',
  'project': 'project',
  'path': '/repo/.sailry/worktrees/feature',
};

void main() {
  test(
    'branch labels match registered paths without changing identity',
    () async {
      final commands = <Map<String, dynamic>>[];
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        command: (kind, data) async {
          expect(kind, 'list_worktrees');
          commands.add(data!);
          return {
            'data': {
              'entries': [
                {'path': '/repo', 'branch': 'main'},
                {'path': feature['path'], 'branch': 'feature/teams'},
              ],
            },
          };
        },
      );
      final branches = DraftBranches(host);
      addTearDown(branches.dispose);
      addTearDown(host.dispose);
      await branches.load(primary);
      expect(commands, [
        {'worktree': 'main'},
      ]);
      expect(branches.label(primary, 'Worktree'), 'main');
      expect(branches.label(feature, 'Worktree'), 'feature/teams');
      expect(feature['id'], 'feature');
    },
  );

  test(
    'detached checkout shows its head and missing metadata stays neutral',
    () async {
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        command: (_, _) async => {
          'data': {
            'entries': [
              {'path': '/repo', 'branch': null, 'head': '123456789abcdef'},
            ],
          },
        },
      );
      final branches = DraftBranches(host);
      addTearDown(branches.dispose);
      addTearDown(host.dispose);
      await branches.load(primary);
      expect(branches.label(primary, 'Worktree'), '1234567');
      expect(branches.label(feature, 'Worktree'), 'Worktree');
      await branches.load(null);
      expect(branches.label(primary, 'Worktree'), 'Worktree');
    },
  );

  test('late metadata cannot replace another project', () async {
    final first = Completer<Map<String, dynamic>>();
    final second = Completer<Map<String, dynamic>>();
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (_, data) =>
          data!['worktree'] == 'main' ? first.future : second.future,
    );
    final branches = DraftBranches(host);
    addTearDown(branches.dispose);
    addTearDown(host.dispose);
    final pending = branches.load(primary);
    final other = {'id': 'other', 'project': 'other', 'path': '/other'};
    final latest = branches.load(other);
    second.complete({
      'data': {
        'entries': [
          {'path': '/other', 'branch': 'next'},
        ],
      },
    });
    await latest;
    first.complete({
      'data': {
        'entries': [
          {'path': '/repo', 'branch': 'old'},
        ],
      },
    });
    await pending;
    expect(branches.label(other, 'Worktree'), 'next');
    expect(branches.label(primary, 'Worktree'), 'Worktree');
  });

  test('disposed pickers ignore pending metadata', () async {
    final reply = Completer<Map<String, dynamic>>();
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (_, _) => reply.future,
    );
    addTearDown(host.dispose);
    final branches = DraftBranches(host);
    final pending = branches.load(primary);
    branches.dispose();
    reply.complete({
      'data': {
        'entries': [
          {'path': '/repo', 'branch': 'main'},
        ],
      },
    });
    await pending;
    expect(branches.label(primary, 'Worktree'), 'Worktree');
  });

  test('failed metadata remains retryable', () async {
    var failed = true;
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (_, _) async {
        if (failed) throw const CommandFailure('unavailable');
        return {
          'data': {
            'entries': [
              {'path': '/repo', 'branch': 'main'},
            ],
          },
        };
      },
    );
    final branches = DraftBranches(host);
    addTearDown(branches.dispose);
    addTearDown(host.dispose);
    await branches.load(primary);
    expect(branches.error, isA<CommandFailure>());
    expect(branches.label(primary, 'Worktree'), 'Worktree');
    failed = false;
    await branches.load(primary);
    expect(branches.error, isNull);
    expect(branches.label(primary, 'Worktree'), 'main');
  });
}
