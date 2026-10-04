import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final sessions = jsonDecode(env['SAILRY_SESSIONS']!) as List;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);

  Future<void> reopen() async {
    await connection.close();
    connection.dispose();
    await controller.close();
    controller.dispose();
    controller = await Controller.open(path: path, internet: false, relays: []);
    connection = await controller.connect(address: address);
  }

  for (final (index, mode) in ['plan', 'code'].indexed) {
    final session = sessions[index];
    final snapshot = (await execute(connection, 'snapshot', null))['data'];
    final original = (snapshot['sessions'] as List).singleWhere(
      (item) => item['id'] == session,
    );
    final config = {...original['config'], 'mode': mode, 'permission': 'full'};
    final set = await connection.prepare(
      command: jsonEncode({
        'kind': 'set_session_config',
        'data': {
          'session': session,
          'expected_revision': original['revision'],
          'config': config,
        },
      }),
    );
    final configured = await connection.execute(request: set);
    final revision = jsonDecode(configured)['Ok']['data']['revision'];
    final queue = await connection.prepare(
      command: jsonEncode({
        'kind': 'queue_turn',
        'data': {
          'session': session,
          'expected_revision': revision,
          'message': {
            'text': 'Use only the tools available in this mode',
            'attachments': [],
          },
        },
      }),
    );
    final queued = await connection.execute(request: queue);
    final turn = jsonDecode(queued)['Ok']['data'];
    check(turn['config']['mode'] == mode, 'queued work mode is frozen');
    final next = mode == 'plan' ? 'code' : 'plan';
    final revised = (await execute(connection, 'set_session_config', {
      'session': session,
      'expected_revision': revision,
      'config': {...config, 'mode': next},
    }))['data'];
    await reopen();
    final latest =
        (await execute(connection, 'snapshot', null))['data']['sessions']
            as List;
    check(
      latest.singleWhere((item) => item['id'] == session)['config']['mode'] ==
          next,
      'reopened controller uses current session mode',
    );
    check(
      await connection.execute(request: set) == configured,
      'configuration retry returns the original result',
    );
    final conflict = await connection.prepare(
      command: jsonEncode({
        'kind': 'set_session_config',
        'data': {
          'session': session,
          'expected_revision': revision,
          'config': config,
        },
      }),
    );
    check(
      jsonDecode(await connection.execute(request: conflict))['Err']['code'] ==
          'revision_conflict',
      'stale mode change is rejected',
    );
    var updates = await connection.watchConversation(session: session);
    await until(updates, (_) => true);
    await execute(connection, 'start_queued_turn', {'turn': turn['id']});
    final completed = await until(
      updates,
      (snapshot) => (snapshot['page']['runs'] as List).any(
        (run) =>
            run['turn'] == turn['id'] &&
            ['completed', 'failed'].contains(run['status']),
      ),
    );
    final page = completed['snapshot']['page'];
    check(
      page['runs'].single['revision'] == revision,
      'execution keeps the admitted work mode',
    );
    if (mode == 'plan') {
      check(
        (page['approvals'] as List).isEmpty,
        'planning cannot authorize a mutation',
      );
      final read = await connection.prepare(
        command: jsonEncode({
          'kind': 'read_file',
          'data': {'worktree': original['worktree'], 'path': 'mode.txt'},
        }),
      );
      check(
        jsonDecode(await connection.execute(request: read))['Err']['code'] ==
            'not_found',
        'planning did not create the file',
      );
    } else {
      check(
        page['runs'].single['status'] == 'completed',
        'coding performs the frozen write',
      );
      check(
        page['approvals'].single['source'] == 'full',
        'coding keeps its permission',
      );
      final read = (await execute(connection, 'read_file', {
        'worktree': original['worktree'],
        'path': 'mode.txt',
      }))['data'];
      check(
        read['text'] == 'Coding after planning 中文 🙂',
        'coding writes the exact requested content',
      );
    }
    check(
      await connection.execute(request: queue) == queued,
      'queue retry does not replay work',
    );
    await updates.close();
    updates.dispose();
    await reopen();
    updates = await connection.watchConversation(session: session);
    final restored = await until(updates, (_) => true);
    check(
      jsonEncode(restored['snapshot']['page']) == jsonEncode(page),
      'history survives reconnect',
    );
    final restoredSessions =
        (await execute(connection, 'snapshot', null))['data']['sessions']
            as List;
    final current = restoredSessions.singleWhere(
      (item) => item['id'] == session,
    );
    check(
      current['revision'] == revised['revision'] &&
          current['config']['mode'] == next,
      'retry and execution do not overwrite current mode',
    );
    await updates.close();
    updates.dispose();
  }
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI frozen planning and coding modes passed');
}
