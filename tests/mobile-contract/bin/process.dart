import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' show hasPending, pending, part;
import 'conversation.dart' show check, execute, until;
import 'attachments/support.dart'
    show uploadAttachment, prepareDownload, downloadAttachment;

Map<String, dynamic> result(Map<String, dynamic> view, int index) =>
    part(view, view['calls'][index]['response'])['result']['data'];

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final sessions = jsonDecode(env['SAILRY_SESSIONS']!) as List;
  var session = sessions[0] as String;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  var updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  final workspace = (await execute(connection, 'snapshot', null))['data'];
  final worktree = (workspace['sessions'] as List).singleWhere(
    (item) => item['id'] == session,
  )['worktree'];

  Future<void> close() async {
    await updates.close();
    updates.dispose();
    await connection.close();
    connection.dispose();
    await controller.close();
    controller.dispose();
  }

  Future<Map<String, dynamic>> reopen() async {
    await close();
    controller = await Controller.open(path: path, internet: false, relays: []);
    connection = await controller.connect(address: address);
    updates = await connection.watchConversation(session: session);
    return until(updates, (_) => true);
  }

  final attachment = await uploadAttachment(
    connection,
    worktree,
    Uint8List.fromList([0, 255, 1, 2]),
    name: 'input.bin',
    mediaType: 'application/octet-stream',
  );
  await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': 1,
    'message': {
      'text': 'Run the fixture commands',
      'attachments': [attachment['id']],
    },
  });
  final waiting = await until(updates, hasPending);
  final first = pending(waiting);
  final source = part(waiting, waiting['calls'][0]['source']);
  final text = List.filled(
    100,
    '完整输出 中文 🙂\n```rust\nlet value = 1;\n```\n',
  ).join();
  check(
    source['name'] == env['SAILRY_RUN_TOOL'] &&
        source['arguments']['command'].contains(text),
    'full literal command crosses FFI',
  );
  check(
    source['arguments']['cwd'] == '' &&
        source['arguments']['timeout_ms'] == 60000,
    'execution options remain attached to the exact call',
  );
  final resumed = await reopen();
  check(
    pending(resumed)['id'] == first['id'],
    'reopening does not authorize execution',
  );
  final countRequest = await connection.prepare(
    command: jsonEncode({
      'kind': 'read_file',
      'data': {'worktree': worktree, 'path': 'count.txt'},
    }),
  );
  check(
    jsonDecode(
          await connection.execute(request: countRequest),
        )['Err']?['code'] ==
        'not_found',
    'pending command has not run',
  );
  final allow = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_approval',
      'data': {
        'session': session,
        'approval': first['id'],
        'decision': 'approve',
      },
    }),
  );
  final receipt = await connection.execute(request: allow);
  check(
    jsonDecode(receipt)['Ok']?['data']?['state'] == 'approved',
    'command approval is durable',
  );
  final next = await until(
    updates,
    (snapshot) =>
        hasPending(snapshot) &&
        (snapshot['page']['approvals'] as List).length == 2,
  );
  final fileRequest = await connection.prepare(
    command: jsonEncode({
      'kind': 'run_command',
      'data': {
        'turn': first['turn'],
        'command': 'od -An -tx1 "\$SAILRY_ATTACHMENTS/${attachment['id']}"',
        'cwd': '',
        'timeout_ms': 1000,
        'background': false,
        'attachments': [attachment['id']],
      },
    }),
  );
  final fileReceipt = await connection.execute(request: fileRequest);
  check(
    (jsonDecode(fileReceipt)['Ok']['data']['stdout']['text'] as String)
            .trim()
            .split(RegExp(r'\s+'))
            .join(' ') ==
        '00 ff 01 02',
    'command reads the complete attachment on its execution Node: $fileReceipt',
  );
  check(
    await connection.execute(request: fileRequest) == fileReceipt,
    'attachment command uses its durable receipt',
  );
  final downloaded = await downloadAttachment(
    connection,
    await prepareDownload(connection, attachment),
  );
  check(
    downloaded.join(',') == '0,255,1,2',
    'command input does not change the original attachment',
  );
  final completed = result(next, 0);
  check(
    completed['outcome']['kind'] == 'exited' &&
        completed['outcome']['data'] == 7,
    'nonzero exit crosses FFI without changing tool response state',
  );
  check(
    completed['stdout']['text'] == text &&
        !completed['stdout']['truncated'] &&
        !completed['stdout']['invalid_utf8'],
    'full Unicode stdout is retained',
  );
  check(
    completed['stderr']['text'] == 'failure detail�' &&
        completed['stderr']['invalid_utf8'],
    'stderr encoding is reported independently',
  );
  await execute(connection, 'resolve_approval', {
    'session': session,
    'approval': pending(next)['id'],
    'decision': 'approve',
  });
  final third = await until(
    updates,
    (snapshot) =>
        hasPending(snapshot) &&
        (snapshot['page']['approvals'] as List).length == 3,
  );
  check(
    result(third, 1)['outcome']['kind'] == 'timed_out' &&
        result(third, 1)['stdout']['text'] == 'before timeout\n',
    'timeout retains partial output',
  );
  await execute(connection, 'resolve_approval', {
    'session': session,
    'approval': pending(third)['id'],
    'decision': 'deny',
  });
  final finished = await until(
    updates,
    (snapshot) => snapshot['page']['runs'].single['status'] == 'completed',
  );
  check(
    finished['calls'][2]['approval']['state'] == 'denied' &&
        part(finished, finished['calls'][2]['response'])['result']['error'] !=
            null,
    'denied command is not reported as an executed command',
  );
  final restored = await reopen();
  check(
    jsonEncode(restored['snapshot']['page']) ==
        jsonEncode(finished['snapshot']['page']),
    'read-only reopen preserves command outcomes',
  );
  check(
    await connection.execute(request: allow) == receipt,
    'stable approval retry preserves original receipt',
  );
  check(
    jsonDecode(
          await connection.execute(request: countRequest),
        )['Ok']['data']['text'] ==
        'x',
    'receipt retry and history replay do not execute again',
  );

  await execute(connection, 'write_file', {
    'worktree': worktree,
    'path': 'stop-mode',
    'text': '',
    'expected_revision': null,
  });
  await updates.close();
  updates.dispose();
  session = sessions[1] as String;
  updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': 1,
    'message': {'text': 'Stop an executing command', 'attachments': []},
  });
  final stopping = pending(await until(updates, hasPending));
  await execute(connection, 'resolve_approval', {
    'session': session,
    'approval': stopping['id'],
    'decision': 'approve',
  });
  final readyRequest = await connection.prepare(
    command: jsonEncode({
      'kind': 'read_file',
      'data': {'worktree': worktree, 'path': 'ready.txt'},
    }),
  );
  final deadline = DateTime.now().add(const Duration(seconds: 5));
  while (jsonDecode(await connection.execute(request: readyRequest))['Ok'] ==
      null) {
    check(DateTime.now().isBefore(deadline), 'command start deadline');
    await Future<void>.delayed(const Duration(milliseconds: 10));
  }
  final running = await reopen();
  check(
    running['snapshot']['page']['runs'].single['status'] == 'running' &&
        running['calls'][0]['approval']['state'] == 'approved',
    'closing the controller does not stop approved execution',
  );
  await execute(connection, 'stop_turn', {'turn': stopping['turn']});
  final stopped = await until(
    updates,
    (snapshot) => snapshot['page']['runs'].single['status'] == 'cancelled',
  );
  check(
    stopped['calls'][0]['state'] == 'interrupted' &&
        stopped['calls'][0]['response'] == null,
    'stop does not invent a canonical tool result',
  );
  check(
    stopped['calls'][0]['approval']['state'] == 'approved',
    'stopping does not undo the prior authorization',
  );
  check(
    jsonDecode(
          await connection.execute(request: countRequest),
        )['Ok']['data']['text'] ==
        'xx',
    'each explicit session executes once',
  );
  await close();
  RustLib.dispose();
  print('Dart FFI command outcomes, denial, stop and resume passed');
}
