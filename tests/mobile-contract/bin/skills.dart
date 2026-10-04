import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' as approvals;
import 'questions.dart' as questions;
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
        'message': {
          'text': 'Use the installed analysis skill',
          'attachments': [],
        },
      },
    }),
  );
  final admitted = await connection.execute(request: submit);
  check(jsonDecode(admitted)['Ok'] != null, 'host admits the turn');
  final references = jsonDecode(admitted)['Ok']['data']['plugins'];
  check(
    jsonEncode((references as List).map((value) => value['name']).toList()) ==
        jsonEncode(['commands', 'example']),
    'new input uses the execution Node package',
  );
  final installed = references.singleWhere(
    (value) => value['name'] == 'example',
  );
  final waiting = await until(updates, questions.hasPending);
  final first = questions.pending(waiting);
  final calls = waiting['calls'] as List;
  check(
    calls.length == 3,
    'skill loads and references precede the user question',
  );
  final loaded = approvals.part(waiting, calls[0]['response'])['result'];
  final body =
      '---\nname: analysis\ndescription: Analyze project data\nallowed-tools: run_command\n---\nLiteral {missing_state}\n${List.filled(1500, '完整内容🙂').join()}\nComplete end\n';
  check(
    loaded['content'] == body,
    'full host skill instruction crosses FFI without truncation',
  );
  check(
    loaded['directory'].contains(installed['digest']) &&
        !loaded['directory'].startsWith(path),
    'skill resource path belongs to the execution Node',
  );
  check(
    approvals.part(waiting, calls[1]['response'])['result']['content'] ==
        'Guide 1.0.0 中文 🙂',
    'resource reads use installed bytes, not modified source',
  );
  check(
    (waiting['snapshot']['page']['approvals'] as List).isEmpty,
    'skill reading does not request execution permission',
  );
  final resumed = await reopen();
  check(
    questions.pending(resumed)['id'] == first['id'],
    'ordinary question survives controller shutdown',
  );
  final answer = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_question',
      'data': {
        'session': session,
        'question': first['id'],
        'response': {
          'kind': 'answer',
          'data': questions.text(' 第一行 中文 🙂\nSecond line '),
        },
      },
    }),
  );
  await connection.execute(request: answer);
  final pendingScript = await until(updates, approvals.hasPending);
  final script = approvals.pending(pendingScript);
  check(
    script['source'] == 'user',
    'allowed-tools does not auto-approve the script',
  );
  final scriptCall = (pendingScript['calls'] as List).last;
  check(
    approvals.part(pendingScript, scriptCall['source'])['name'] ==
        Platform.environment['SAILRY_RUN_TOOL'],
    'scripts use the existing command tool',
  );
  final command = approvals.part(
    pendingScript,
    scriptCall['source'],
  )['arguments']['command'];
  check(
    command.contains(loaded['directory']),
    'approved command targets the installed skill directory',
  );
  final absent = await connection.prepare(
    command: jsonEncode({
      'kind': 'read_file',
      'data': {'worktree': configured['worktree'], 'path': 'script-count.txt'},
    }),
  );
  check(
    jsonDecode(await connection.execute(request: absent))['Err']?['code'] ==
        'not_found',
    'pending command has not executed',
  );
  final reopened = await reopen();
  check(
    approvals.pending(reopened)['id'] == script['id'],
    'command approval survives reconnect',
  );
  final approve = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_approval',
      'data': {
        'session': session,
        'approval': script['id'],
        'decision': 'approve',
      },
    }),
  );
  await connection.execute(request: approve);
  final denyView = await until(
    updates,
    (snapshot) =>
        approvals.hasPending(snapshot) &&
        (snapshot['page']['approvals'] as List).length == 2,
  );
  await execute(connection, 'resolve_approval', {
    'session': session,
    'approval': approvals.pending(denyView)['id'],
    'decision': 'deny',
  });
  final finished = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'completed',
  );
  final completed = finished['calls'] as List;
  check(
    completed.length == 5 &&
        completed.every((call) => call['state'] == 'returned'),
    'all skill interactions use shared canonical calls',
  );
  check(
    approvals.part(
          finished,
          completed[3]['response'],
        )['result']['data']['stdout']['text'] ==
        'script output 中文 🙂',
    'approved script returns real execution output',
  );
  check(
    approvals.part(finished, completed[4]['response'])['result']['error'] !=
        null,
    'denial does not claim execution',
  );
  final written = await execute(connection, 'read_file', {
    'worktree': configured['worktree'],
    'path': 'script-count.txt',
  });
  check(written['data']['text'] == 'x', 'script executes once');
  final restored = await reopen();
  check(
    jsonEncode(restored['snapshot']['page']) ==
        jsonEncode(finished['snapshot']['page']),
    'skill history recovers without loading or executing again',
  );
  check(
    await connection.execute(request: submit) == admitted,
    'original submission receipt is retained',
  );
  check(
    jsonDecode(
          await connection.execute(request: approve),
        )['Ok']['data']['state'] ==
        'approved',
    'lost approval receipt recovers',
  );
  check(
    jsonDecode(
          await connection.execute(request: answer),
        )['Ok']['data']['state']['data']['data'] ==
        ' 第一行 中文 🙂\nSecond line ',
    'lost answer receipt recovers',
  );
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  check(
    jsonEncode((snapshot['turns'] as List).single['plugins']) ==
        jsonEncode(references),
    'admitted turn retains host plugin references',
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
      'text': 'Stop this skill task at its question',
      'attachments': [],
    },
  });
  final stopped = questions.pending(await until(updates, questions.hasPending));
  await execute(connection, 'stop_turn', {'turn': stopped['turn']});
  await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).single['status'] == 'cancelled',
  );
  final late = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_question',
      'data': {
        'session': session,
        'question': stopped['id'],
        'response': {'kind': 'answer', 'data': questions.text('Too late')},
      },
    }),
  );
  check(
    jsonDecode(await connection.execute(request: late))['Err']?['code'] ==
        'conflict',
    'late answer cannot resume a stopped skill task',
  );
  await close();
  RustLib.dispose();
  print(
    'Dart FFI host skills, resources, questions, command approval and recovery passed',
  );
}
