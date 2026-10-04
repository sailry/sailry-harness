import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' as approvals;
import 'conversation.dart' show check, execute, until;

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

  final workspace = (await execute(connection, 'snapshot', null))['data'];
  final configured = (workspace['sessions'] as List).singleWhere(
    (value) => value['id'] == session,
  );
  final submit = await connection.prepare(
    command: jsonEncode({
      'kind': 'submit_turn',
      'data': {
        'session': session,
        'expected_revision': configured['revision'],
        'message': {'text': 'Use the configured tools', 'attachments': []},
      },
    }),
  );
  final admitted = await connection.execute(request: submit);
  check(jsonDecode(admitted)['Ok'] != null, 'host admits the turn');
  final references = jsonDecode(admitted)['Ok']['data']['plugins'];
  check(
    references.length == 1 && references[0]['name'] == 'example',
    'new input uses the execution Node package',
  );
  final waiting = await until(updates, approvals.hasPending);
  final pending = approvals.pending(waiting);
  final calls = waiting['calls'] as List;
  check(calls.length == 2, 'read completes before the write approval');
  final output = approvals.part(
    waiting,
    calls[0]['response'],
  )['result']['output'];
  check(
    output['version'] == 'Installed version 中文 🙂' &&
        output['root'].contains(references[0]['digest']) &&
        !output['root'].startsWith(path),
    'MCP uses the installed host resources, not modified source or controller',
  );
  check(
    output['data'] == output['cwd'] &&
        output['text'].endsWith(r'|${HOME}|中文 🙂') &&
        output['provider_key_present'] == false,
    'server receives declared data paths without model credentials',
  );
  final fullText = List.filled(1500, '完整 MCP 内容🙂').join();
  check(
    approvals.part(waiting, calls[1]['source'])['arguments']['value'] ==
        fullText,
    'full proposed MCP arguments cross FFI',
  );
  check(
    pending['source'] == 'user' &&
        calls[1]['approval']['id'] == pending['id'] &&
        calls[1]['source']['entry'] == pending['entry'] &&
        calls[1]['source']['index'] == pending['index'],
    'approval belongs to the exact shared canonical call',
  );
  final wrong = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_approval',
      'data': {
        'session': sessions[1],
        'approval': pending['id'],
        'decision': 'approve',
      },
    }),
  );
  check(
    jsonDecode(await connection.execute(request: wrong))['Err']?['code'] ==
        'wrong_target',
    'another session cannot approve this MCP call',
  );
  final resumed = await reopen();
  check(
    approvals.pending(resumed)['id'] == pending['id'] &&
        (resumed['calls'] as List).length == 2,
    'controller shutdown leaves the original server call waiting',
  );
  final allow = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_approval',
      'data': {
        'session': session,
        'approval': pending['id'],
        'decision': 'approve',
      },
    }),
  );
  await connection.execute(request: allow);
  final denyView = await until(
    updates,
    (snapshot) =>
        approvals.hasPending(snapshot) &&
        (snapshot['page']['approvals'] as List).length == 2,
  );
  final deny = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_approval',
      'data': {
        'session': session,
        'approval': approvals.pending(denyView)['id'],
        'decision': 'deny',
      },
    }),
  );
  await connection.execute(request: deny);
  final finished = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'completed',
  );
  final completed = finished['calls'] as List;
  check(
    completed.length == 3 &&
        completed.every((call) => call['state'] == 'returned'),
    'MCP results use ordinary shared tool calls',
  );
  check(
    approvals.part(
          finished,
          completed[1]['response'],
        )['result']['output']['value'] ==
        fullText,
    'complete structured MCP result survives the host and FFI paths',
  );
  check(
    approvals.part(finished, completed[2]['response'])['result']['error'] !=
        null,
    'denied MCP call does not claim execution',
  );
  final restored = await reopen();
  check(
    jsonEncode(restored['snapshot']['page']) ==
        jsonEncode(finished['snapshot']['page']),
    'reopen recovers canonical history without executing tools',
  );
  check(
    await connection.execute(request: submit) == admitted,
    'original submission receipt is retained',
  );
  check(
    jsonDecode(
          await connection.execute(request: allow),
        )['Ok']['data']['state'] ==
        'approved',
    'lost approval receipt recovers without repeating the effect',
  );
  check(
    jsonDecode(
          await connection.execute(request: deny),
        )['Ok']['data']['state'] ==
        'denied',
    'lost denial receipt remains attached to its original call',
  );
  await updates.close();
  updates.dispose();
  session = sessions[1] as String;
  updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': 1,
    'message': {
      'text': 'Stop before calling the write tool',
      'attachments': [],
    },
  });
  final stopped = approvals.pending(await until(updates, approvals.hasPending));
  await reopen();
  await execute(connection, 'stop_turn', {'turn': stopped['turn']});
  final cancelled = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'cancelled',
  );
  check(
    cancelled['calls'][1]['state'] == 'cancelled' &&
        cancelled['calls'][1]['approval']['state'] == 'cancelled',
    'stop closes the pending call and approval',
  );
  final late = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_approval',
      'data': {
        'session': session,
        'approval': stopped['id'],
        'decision': 'approve',
      },
    }),
  );
  check(
    jsonDecode(await connection.execute(request: late))['Err']?['code'] ==
        'conflict',
    'late approval cannot restart the stopped MCP server',
  );
  await close();
  RustLib.dispose();
  print('Dart FFI host MCP results, approval, stop and recovery passed');
}
