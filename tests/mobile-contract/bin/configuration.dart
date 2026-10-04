import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'session_roles.dart' show checkSourceRoles, verifyRoles;

Future<String> send(Connection connection, Map<String, dynamic> session) async {
  final updates = await connection.watchConversation(session: session['id']);
  await until(updates, (_) => true);
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'submit_turn',
      'data': {
        'session': session['id'],
        'expected_revision': session['revision'],
        'message': {'text': 'FFI configuration resume', 'attachments': []},
      },
    }),
  );
  final result = jsonDecode(await connection.execute(request: request));
  check(result['Ok']?['kind'] == 'queued_turn', 'turn admitted over FFI');
  final turn = result['Ok']['data']['id'];
  final complete = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == turn && run['status'] == 'completed',
    ),
  );
  check(
    complete['snapshot']['page']['entries'].last['parts'][0]['data'] ==
        'answer-${session['config']['model']}',
    'session-owned model executes',
  );
  await updates.close();
  updates.dispose();
  return request;
}

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  final state = (await execute(connection, 'snapshot', null))['data'];
  final session =
      (state['sessions'] as List).singleWhere(
            (session) => session['id'] == env['SAILRY_SESSION'],
          )
          as Map<String, dynamic>;
  check(
    session['config']['model'] == 'ffi-source',
    'source model persists at execution Node',
  );
  check(
    session['profile']['provider']['default_model'] == 'ffi-source',
    'session provider metadata crosses FFI',
  );
  check(
    jsonEncode(session['config']['credential']['node']) ==
        jsonEncode(state['node']),
    'credential reference belongs to execution Node',
  );
  check(
    !jsonEncode(state).contains('isolated-ffi-configuration-credential'),
    'no secret in shared snapshot',
  );
  checkSourceRoles(session, state as Map<String, dynamic>);
  final request = await send(connection, session);
  final expected = await connection.execute(request: request);
  final created =
      (await execute(connection, 'create_session', {
            'project': session['project'],
            'worktree': session['worktree'],
            'config': null,
          }))['data']
          as Map<String, dynamic>;
  check(
    created['config']['model'] == 'ffi-default',
    'Mobile new session uses execution defaults',
  );
  check(
    created['profile'] == null,
    'new default session is not another imported copy',
  );
  await send(connection, created);
  await verifyRoles(connection, session, created);
  await controller.close();
  connection.dispose();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  check(
    await connection.execute(request: request) == expected,
    'reconnect preserves request identity',
  );
  final restored = (await execute(connection, 'snapshot', null))['data'];
  check(
    (restored['sessions'] as List).length == 2,
    'reconnect does not create another session',
  );
  final resumed = (restored['sessions'] as List).singleWhere(
    (entry) => entry['id'] == session['id'],
  );
  check(
    resumed['revision'] == 3 &&
        resumed['roles']['profiles'].single['key'] == 'target-review',
    'reopened controller reads execution-owned role revision',
  );
  check(
    (restored['roles'] as List).isEmpty,
    'catalog removal does not remove session roles',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI session configuration and independent resume passed');
}
