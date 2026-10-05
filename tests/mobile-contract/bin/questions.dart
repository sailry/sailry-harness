import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' show part;
import 'conversation.dart' show check, execute, until;

bool hasPending(Map<String, dynamic> snapshot) =>
    (snapshot['page']['questions'] as List).any(
      (question) => question['state']['kind'] == 'pending',
    );

Map<String, dynamic> pending(Map<String, dynamic> view) =>
    (view['snapshot']['page']['questions'] as List).singleWhere(
      (question) => question['state']['kind'] == 'pending',
    );

Map<String, dynamic> text(String value) => {'kind': 'text', 'data': value};

Map<String, dynamic> choices(List<int> selected, [String? other]) => {
  'kind': 'choices',
  'data': {'selected': selected, 'other': other},
};

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
  var session = sessions[0] as String;
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

  Future<String> prepare(
    String id,
    Map<String, dynamic> answer, [
    String? target,
  ]) => connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_question',
      'data': {
        'session': target ?? session,
        'question': id,
        'response': {'kind': 'answer', 'data': answer},
      },
    }),
  );

  Future<void> reject(
    String id,
    Map<String, dynamic> answer,
    String code, [
    String? target,
  ]) async {
    final request = await prepare(id, answer, target);
    final result = jsonDecode(await connection.execute(request: request));
    check(
      result['Err']?['code'] == code,
      'invalid or stale answer is rejected',
    );
  }

  Future<Map<String, dynamic>> next(int count) => until(
    updates,
    (snapshot) =>
        hasPending(snapshot) &&
        (snapshot['page']['questions'] as List).length == count,
  );

  final workspace = (await execute(connection, 'snapshot', null))['data'];
  final original = (workspace['sessions'] as List).singleWhere(
    (value) => value['id'] == session,
  );
  final configured = (await execute(connection, 'set_session_config', {
    'session': session,
    'expected_revision': original['revision'],
    'config': {...original['config'], 'permission': 'full'},
  }))['data'];
  await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': configured['revision'],
    'message': {'text': 'Ask the fixture questions', 'attachments': []},
  });
  final waiting = await next(1);
  final first = pending(waiting);
  final call = (waiting['calls'] as List).single;
  check(
    call['state'] == 'running' &&
        call['question']['state']['kind'] == 'pending',
    'admitted question tool waits for input',
  );
  check(
    call['question']['id'] == first['id'] &&
        call['source']['entry'] == first['entry'] &&
        call['source']['index'] == first['index'],
    'shared pending question references its exact canonical call',
  );
  final spec = part(waiting, call['source'])['arguments'];
  check(
    spec['prompt'] == '补充要求 中文 🙂' &&
        spec['input']['multiline'] == true &&
        spec['input']['max_bytes'] == 64 &&
        (waiting['snapshot']['page']['approvals'] as List).isEmpty,
    'full permission still waits for user input without approval',
  );
  await reject(first['id'], text('answer'), 'wrong_target', sessions[1]);
  await reject(first['id'], choices([0]), 'invalid_request');
  await reject(first['id'], text('   '), 'invalid_request');
  await reject(
    first['id'],
    text(List.filled(22, '中').join()),
    'invalid_request',
  );
  final resumed = await reopen();
  check(
    pending(resumed)['id'] == first['id'],
    'pending question survives reopen',
  );
  const fullText = ' 第一行 中文 🙂\nSecond line ';
  final answer = await prepare(first['id'], text(fullText));
  // Discard the application receipt, then recover with the exact same request.
  await connection.execute(request: answer);
  final secondView = await next(2);
  final second = pending(secondView);
  final receipt = await connection.execute(request: answer);
  check(
    jsonDecode(receipt)['Ok']['data']['state']['data']['data'] == fullText,
    'stable retry recovers the original answer without trimming',
  );
  final page = (await execute(connection, 'read_conversation', {
    'session': session,
    'before': null,
    'limit': 100,
  }))['data']['page'];
  check(
    page['questions'][1]['id'] == second['id'] &&
        page['questions'][1]['state']['kind'] == 'pending',
    'old answer cannot resolve the next question',
  );
  await reject(first['id'], text('different'), 'conflict');
  await reject(second['id'], choices([]), 'invalid_request');
  await reject(second['id'], choices([0, 1]), 'invalid_request');
  await reject(second['id'], choices([0], 'extra'), 'invalid_request');
  final selected = await prepare(second['id'], choices([], ' 自定义 中文 🙂 '));
  await connection.execute(request: selected);
  final thirdView = await next(3);
  final third = pending(thirdView);
  await reject(third['id'], choices([0, 0]), 'invalid_request');
  await reject(third['id'], choices([3]), 'invalid_request');
  final multiple = await prepare(third['id'], choices([0, 2], '其他 🙂'));
  await connection.execute(request: multiple);
  final fourthView = await next(4);
  final fourth = pending(fourthView);
  await reject(fourth['id'], text('first\nsecond'), 'invalid_request');
  await execute(connection, 'resolve_question', {
    'session': session,
    'question': fourth['id'],
    'response': {'kind': 'cancel'},
  });
  final finished = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'completed',
  );
  final calls = finished['calls'] as List;
  final results = calls
      .map((call) => part(finished, call['response'])['result'])
      .toList();
  check(
    calls.length == 4 &&
        calls.every((call) => call['state'] == 'returned') &&
        results.take(3).every((result) => result['status'] == 'answered') &&
        results[0]['answer'] == fullText &&
        jsonEncode(results[1]['answers']) == jsonEncode([' 自定义 中文 🙂 ']) &&
        jsonEncode(results[2]['answers']) ==
            jsonEncode(['代码', '测试', '其他 🙂']) &&
        results[3]['status'] == 'cancelled',
    'all answers and cancellation return through canonical ADK results',
  );
  final restored = await reopen();
  check(
    jsonEncode(restored['snapshot']['page']) ==
            jsonEncode(finished['snapshot']['page']) &&
        jsonEncode(restored['calls']) == jsonEncode(calls),
    'reopening restores history without replay',
  );
  check(
    await connection.execute(request: answer) == receipt,
    'receipt survives reconnect',
  );
  await updates.close();
  updates.dispose();
  session = sessions[1] as String;
  updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': 1,
    'message': {'text': 'Stop before answering', 'attachments': []},
  });
  final stopped = pending(await next(1));
  await execute(connection, 'stop_turn', {'turn': stopped['turn']});
  final cancelled = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'cancelled',
  );
  final closed = (cancelled['calls'] as List).single;
  check(
    closed['state'] == 'cancelled' &&
        closed['question']['state']['kind'] == 'cancelled' &&
        closed['response'] == null,
    'stop closes the question without inventing a tool result',
  );
  await reject(stopped['id'], text('too late'), 'conflict');
  await close();
  RustLib.dispose();
  print('Dart FFI questions, answer validation, retries and stop passed');
}
