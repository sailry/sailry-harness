import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' show part;
import 'conversation.dart' show check, execute, until;
import 'questions.dart' show hasPending, pending;
import 'mcp_oauth.dart' show authorizeMcp;

Future<void> main() async {
  final env = Platform.environment;
  final url = env['SAILRY_MCP_URL'] == '1';
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
  if (env['SAILRY_MCP_OAUTH'] == '1') await authorizeMcp(connection);
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

  Future<String> submit() async {
    final snapshot = (await execute(connection, 'snapshot', null))['data'];
    final selected = (snapshot['sessions'] as List).singleWhere(
      (value) => value['id'] == session,
    );
    final output = await execute(connection, 'submit_turn', {
      'session': session,
      'expected_revision': selected['revision'],
      'message': {'text': 'Prepare a report', 'attachments': []},
    });
    return output['data']['id'] as String;
  }

  Future<String> resolve(String id, Map<String, dynamic> response) =>
      connection.prepare(
        command: jsonEncode({
          'kind': 'resolve_question',
          'data': {'session': session, 'question': id, 'response': response},
        }),
      );
  await submit();
  final waiting = await until(updates, hasPending);
  final question = pending(waiting);
  final call = (waiting['calls'] as List).singleWhere(
    (call) => call['question']?['id'] == question['id'],
  );
  final input = part(waiting, call['source'])['arguments']['input'];
  check(
    url
        ? input['kind'] == 'url' &&
              input['url'] == 'https://example.invalid/continue?task=fixture' &&
              (input['elicitation_id'] as String).isNotEmpty
        : input['kind'] == 'form' && (input['fields'] as List).length == 8,
    'MCP input crosses the shared projection',
  );
  final resumed = await reopen();
  check(
    pending(resumed)['id'] == question['id'],
    'controller closure does not answer or cancel host input',
  );
  final values = {
    'title': 'Report 中文🙂',
    'copies': 2,
    'ratio': 1.5,
    'publish': false,
    'format': 'md',
    'sections': ['summary'],
  };
  if (!url) {
    final invalid = await resolve(question['id'], {
      'kind': 'answer',
      'data': {
        'kind': 'form',
        'data': {...values, 'email': 'invalid email'},
      },
    });
    check(
      jsonDecode(await connection.execute(request: invalid))['Err']?['code'] ==
          'invalid_request',
      'Node validates original MCP formats',
    );
  }
  final answer = await resolve(question['id'], {
    'kind': 'answer',
    'data': url ? {'kind': 'opened'} : {'kind': 'form', 'data': values},
  });
  final receipt = await connection.execute(request: answer);
  check(jsonDecode(receipt)['Ok'] != null, 'input accepted');
  final second = await until(
    updates,
    (snapshot) =>
        hasPending(snapshot) &&
        (snapshot['page']['questions'] as List).length == 2,
  );
  final decline = await resolve(pending(second)['id'], {'kind': 'decline'});
  await connection.execute(request: decline);
  final completed = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'completed',
  );
  check(
    completed['snapshot']['page']['questions'][1]['state']['kind'] ==
        'declined',
    'decline stays distinct from cancellation',
  );
  final firstResult = (completed['calls'] as List).firstWhere(
    (call) => call['name'] != 'mcp_elicitation',
  );
  final output = part(completed, firstResult['response'])['result']['output'];
  check(
    url
        ? output['action'] == 'accept' && !output.containsKey('content')
        : output['content']['publish'] == false,
    'URL consent has no form content; false remains an explicit form answer',
  );
  await reopen();
  check(
    await connection.execute(request: answer) == receipt,
    'accepted reply is idempotent after reopening',
  );
  final continued = await submit();
  for (var count = 3; count <= 4; count++) {
    final next = await until(
      updates,
      (snapshot) =>
          hasPending(snapshot) &&
          (snapshot['page']['questions'] as List).length == count,
    );
    final question = pending(next);
    check(
      question['turn'] == continued,
      'reused connection binds input to the new turn',
    );
    final response = await resolve(question['id'], {'kind': 'decline'});
    await connection.execute(request: response);
  }
  await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == continued && run['status'] == 'completed',
    ),
  );
  session = sessions[1] as String;
  await reopen();
  final turn = await submit();
  await until(updates, hasPending);
  await execute(connection, 'stop_turn', {'turn': turn});
  await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'cancelled',
  );
  await close();
  RustLib.dispose();
}
