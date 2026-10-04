import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;

bool hasPending(Map<String, dynamic> snapshot) =>
    (snapshot['page']['approvals'] as List).any(
      (approval) => approval['state'] == 'pending',
    );

Map<String, dynamic> pending(Map<String, dynamic> view) =>
    (view['snapshot']['page']['approvals'] as List).singleWhere(
      (approval) => approval['state'] == 'pending',
    );

Map<String, dynamic> part(
  Map<String, dynamic> view,
  Map<String, dynamic> reference,
) => (view['snapshot']['page']['entries'] as List).singleWhere(
  (entry) => entry['id'] == reference['entry'],
)['parts'][reference['index']]['data'];

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final sessions = jsonDecode(env['SAILRY_SESSIONS']!) as List;
  final session = sessions[0] as String;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  final workspace = (await execute(connection, 'snapshot', null))['data'];
  final worktree = (workspace['sessions'] as List).singleWhere(
    (value) => value['id'] == session,
  )['worktree'];
  var updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': 1,
    'message': {'text': 'Write the requested files', 'attachments': []},
  });
  final waiting = await until(updates, hasPending);
  final first = pending(waiting);
  final call = (waiting['calls'] as List).single;
  check(
    call['state'] == 'waiting' && call['approval']['id'] == first['id'],
    'shared pending approval is attached to the exact call',
  );
  check(
    first['entry'] == call['source']['entry'] &&
        first['index'] == call['source']['index'],
    'approval and call reference the same canonical part',
  );
  final arguments = part(waiting, call['source'])['arguments'];
  final fullText = List.filled(
    100,
    '完整的审批正文 🙂\nA literal sample: ```rust\nlet value = 1;\n```\n',
  ).join();
  check(
    arguments['text'] == fullText && arguments['expected_revision'] == null,
    'full proposed content crosses FFI without truncation',
  );
  final wrong = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_approval',
      'data': {
        'session': sessions[1],
        'approval': first['id'],
        'decision': 'approve',
      },
    }),
  );
  check(
    jsonDecode(await connection.execute(request: wrong))['Err']?['code'] ==
        'wrong_target',
    'another session cannot resolve this call',
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();

  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  updates = await connection.watchConversation(session: session);
  final resumed = await until(updates, hasPending);
  check(
    pending(resumed)['id'] == first['id'],
    'pending decision survives reopen',
  );
  final unwritten = await connection.prepare(
    command: jsonEncode({
      'kind': 'read_file',
      'data': {'worktree': worktree, 'path': 'approved.txt'},
    }),
  );
  check(
    jsonDecode(await connection.execute(request: unwritten))['Err']?['code'] ==
        'not_found',
    'reopening does not authorize the proposed write',
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
  final allowed = await connection.execute(request: allow);
  check(
    jsonDecode(allowed)['Ok']?['data']?['state'] == 'approved',
    'approval receipt crosses FFI',
  );
  final next = await until(
    updates,
    (snapshot) =>
        hasPending(snapshot) &&
        (snapshot['page']['approvals'] as List).length == 2,
  );
  final second = pending(next);
  await execute(connection, 'resolve_approval', {
    'session': session,
    'approval': second['id'],
    'decision': 'deny',
  });
  final finished = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'completed',
  );
  final calls = finished['calls'] as List;
  check(
    calls.length == 2 &&
        calls[0]['approval']['state'] == 'approved' &&
        calls[1]['approval']['state'] == 'denied' &&
        calls.every((call) => call['state'] == 'returned'),
    'approval decisions remain distinct from returned tool state',
  );
  check(
    part(finished, calls[0]['response'])['result']['kind'] == 'file_written',
    'approved write returns actual service result',
  );
  check(
    part(finished, calls[1]['response'])['result']['error'] != null,
    'denial returns to ADK without claiming a write',
  );
  final saved = await execute(connection, 'read_file', {
    'worktree': worktree,
    'path': 'approved.txt',
  });
  check(
    saved['data']['text'] == fullText,
    'approved file contains the full proposal',
  );
  await updates.close();
  updates.dispose();

  final cancelledSession = sessions[1] as String;
  updates = await connection.watchConversation(session: cancelledSession);
  await until(updates, (_) => true);
  await execute(connection, 'submit_turn', {
    'session': cancelledSession,
    'expected_revision': 1,
    'message': {'text': 'Cancel before writing', 'attachments': []},
  });
  final stopped = pending(await until(updates, hasPending));
  await execute(connection, 'stop_turn', {'turn': stopped['turn']});
  final cancelled = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'cancelled',
  );
  check(
    cancelled['calls'][0]['state'] == 'cancelled' &&
        cancelled['calls'][0]['approval']['state'] == 'cancelled',
    'stop closes both the shared call and its pending approval',
  );
  final late = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_approval',
      'data': {
        'session': cancelledSession,
        'approval': stopped['id'],
        'decision': 'approve',
      },
    }),
  );
  check(
    jsonDecode(await connection.execute(request: late))['Err']?['code'] ==
        'conflict',
    'late approval cannot execute a stopped call',
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();

  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  updates = await connection.watchConversation(session: session);
  final restored = await until(updates, (_) => true);
  check(
    jsonEncode(restored['snapshot']['page']) ==
            jsonEncode(finished['snapshot']['page']) &&
        jsonEncode(restored['calls']) == jsonEncode(calls),
    'read-only reopen restores the canonical history and decisions',
  );
  check(
    await connection.execute(request: allow) == allowed,
    'stable retry retains original receipt',
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI approval, denial, stop and resume passed');
}
