import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' show part;
import 'conversation.dart' show check, execute, until;
import 'questions.dart' show hasPending, pending;

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final session = env['SAILRY_SESSION']!;
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

  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final original = (snapshot['sessions'] as List).singleWhere(
    (item) => item['id'] == session,
  );
  final configured = (await execute(connection, 'set_session_config', {
    'session': session,
    'expected_revision': original['revision'],
    'config': {...original['config'], 'mode': 'plan', 'permission': 'full'},
  }))['data'];
  final turn = (await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': configured['revision'],
    'message': {
      'text': 'Propose a plan for the requested file',
      'attachments': [],
    },
  }))['data'];
  final waiting = await until(updates, hasPending);
  final question = pending(waiting);
  final call = (waiting['calls'] as List).single;
  check(
    part(waiting, call['source'])['arguments']['input']['kind'] == 'plan',
    'plan uses the canonical question specification',
  );
  check(
    (waiting['snapshot']['page']['approvals'] as List).isEmpty,
    'full permission still waits for plan review',
  );
  check(
    pending(await reopen())['id'] == question['id'],
    'plan review survives controller reconnect',
  );
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_question',
      'data': {
        'session': session,
        'question': question['id'],
        'response': {
          'kind': 'start_coding',
          'data': {
            'expected_revision': configured['revision'],
            'message': {
              'text': 'Implement the accepted plan 中文 🙂',
              'attachments': [],
            },
          },
        },
      },
    }),
  );
  final receipt = await connection.execute(request: request);
  final output = jsonDecode(receipt)['Ok'];
  check(
    output['kind'] == 'plan_accepted',
    'acceptance returns the admitted coding turn',
  );
  final accepted = output['data'];
  check(
    accepted['question']['state']['data']['data']['turn'] ==
        accepted['turn']['id'],
    'answer references its exact coding turn',
  );
  check(
    accepted['session']['config']['mode'] == 'code' &&
        accepted['turn']['config']['mode'] == 'code',
    'acceptance revises and freezes coding mode',
  );
  final finished = await until(updates, (snapshot) {
    final runs = snapshot['page']['runs'] as List;
    return runs.length == 2 &&
        runs.every((run) => run['status'] == 'completed');
  });
  final page = finished['snapshot']['page'];
  check(
    (page['runs'] as List).singleWhere(
          (run) => run['turn'] == turn['id'],
        )['revision'] ==
        configured['revision'],
    'original planning revision remains immutable',
  );
  final acceptedCall = (finished['calls'] as List).first;
  check(
    part(finished, acceptedCall['response'])['result']['coding_turn'] ==
        accepted['turn']['id'],
    'accepted plan is persisted as an ADK tool result',
  );
  final read = (await execute(connection, 'read_file', {
    'worktree': original['worktree'],
    'path': 'review.txt',
  }))['data'];
  check(
    read['text'] == 'Accepted from Dart 中文 🙂',
    'coding executes through the real Node',
  );
  final restored = await reopen();
  check(
    jsonEncode(restored['snapshot']['page']) == jsonEncode(page),
    'completed plan history survives reconnect',
  );
  check(
    await connection.execute(request: request) == receipt,
    'stable acceptance retry does not create another turn',
  );
  final current =
      (await execute(connection, 'snapshot', null))['data']['sessions'] as List;
  check(
    current.singleWhere((item) => item['id'] == session)['config']['mode'] ==
        'code',
    'shared session configuration records acceptance',
  );
  await close();
  RustLib.dispose();
  print('Dart FFI plan acceptance, coding execution and replay passed');
}
