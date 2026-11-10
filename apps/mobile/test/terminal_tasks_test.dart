import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/terminal_tasks.dart';
import 'package:sailry_mobile/runtime/session.dart';

void main() {
  test('routes captured terminals to their owning close command', () async {
    final requests = <Map<String, dynamic>>[];
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (kind, data) async {
        requests.add({'kind': kind, 'data': data});
        return {};
      },
    );
    addTearDown(host.dispose);
    await closeTerminal(host, {
      'id': 'workspace-terminal',
      'worktree': 'tree',
      'ssh': null,
    });
    await closeTerminal(host, {
      'id': 'ssh-terminal',
      'worktree': null,
      'ssh': 'server',
    });
    expect(requests, [
      {
        'kind': 'close_terminal',
        'data': {'worktree': 'tree', 'terminal': 'workspace-terminal'},
      },
      {
        'kind': 'close_ssh_terminal',
        'data': {'terminal': 'ssh-terminal'},
      },
    ]);
  });

  test('preserves uncertain outcomes without replay', () async {
    var attempts = 0;
    final failure = CommandFailure('outcome_unknown', request: 'request');
    final host = HostConnection.test(
      id: 'host',
      label: 'Host',
      command: (_, _) async {
        attempts++;
        throw failure;
      },
    );
    addTearDown(host.dispose);
    await expectLater(
      closeTerminal(host, {'id': 'ssh-terminal', 'ssh': 'server'}),
      throwsA(same(failure)),
    );
    expect(attempts, 1);
  });
}
