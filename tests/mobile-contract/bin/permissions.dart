import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' show hasPending, pending, part;
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

  Future<Map<String, dynamic>> current() async =>
      ((await execute(connection, 'snapshot', null))['data']['sessions']
              as List)
          .singleWhere((item) => item['id'] == session);

  final unrestrictedApprovals = <String>{};
  for (final (index, mode) in ['ask', 'project', 'full'].indexed) {
    if (index != 0) {
      await updates.close();
      updates.dispose();
      session = sessions[index] as String;
      updates = await connection.watchConversation(session: session);
      await until(updates, (_) => true);
    }
    final original = await current();
    final set = await connection.prepare(
      command: jsonEncode({
        'kind': 'set_session_config',
        'data': {
          'session': session,
          'expected_revision': original['revision'],
          'config': {...original['config'], 'permission': mode},
        },
      }),
    );
    final configured = await connection.execute(request: set);
    final config = jsonDecode(configured)['Ok']['data'];
    check(
      config['config']['permission'] == mode,
      'selected permission is authoritative',
    );
    final queue = await connection.prepare(
      command: jsonEncode({
        'kind': 'queue_turn',
        'data': {
          'session': session,
          'expected_revision': config['revision'],
          'message': {'text': 'Use the fixture tools', 'attachments': []},
        },
      }),
    );
    final queued = await connection.execute(request: queue);
    final turn = jsonDecode(queued)['Ok']['data'];
    final next = mode == 'full' ? 'ask' : 'full';
    final revised = (await execute(connection, 'set_session_config', {
      'session': session,
      'expected_revision': config['revision'],
      'config': {...config['config'], 'permission': next},
    }))['data'];
    final restored = await reopen();
    check(
      restored['snapshot']['page']['queue']['items']
              .single['config_revision'] ==
          config['revision'],
      'queued permission revision survives reconnect',
    );
    check(
      (await current())['config']['permission'] == next,
      'reopened controller resumes current permission',
    );
    check(
      await connection.execute(request: set) == configured,
      'configuration retry returns original receipt',
    );
    check(
      (await current())['revision'] == revised['revision'],
      'old request does not overwrite current configuration',
    );
    await execute(connection, 'start_queued_turn', {'turn': turn['id']});

    if (mode == 'ask') {
      final waiting = await until(updates, hasPending);
      check(
        pending(waiting)['source'] == 'user' &&
            part(waiting, pending(waiting))['name'] == env['SAILRY_WRITE_TOOL'],
        'ask requires explicit file authorization',
      );
      await execute(connection, 'resolve_approval', {
        'session': session,
        'approval': pending(waiting)['id'],
        'decision': 'approve',
      });
    }
    final waiting = await until(
      updates,
      (snapshot) =>
          hasPending(snapshot) &&
          (snapshot['page']['approvals'] as List).length == 3,
    );
    final workspace = waiting['snapshot']['page']['approvals'][1];
    check(
      workspace['source'] == 'sandbox' &&
          workspace['state'] == 'approved' &&
          workspace['access'] == 'workspace' &&
          workspace['can_remember'] == false &&
          part(waiting, workspace)['arguments']['command'] ==
              'printf x >> count.txt',
      'workspace commands run inside the sandbox in every frozen mode',
    );
    final command = pending(waiting);
    check(
      part(waiting, command)['name'] == env['SAILRY_RUN_TOOL'] &&
          part(waiting, command)['arguments']['access'] == 'unrestricted' &&
          command['source'] == 'user' &&
          command['access'] == 'unrestricted' &&
          command['can_remember'] == false &&
          command['prefix'] == null &&
          command['choice'] == null &&
          unrestrictedApprovals.add(command['id'] as String),
      'unrestricted commands need a fresh decision in every frozen mode',
    );
    final before = await connection.prepare(
      command: jsonEncode({
        'kind': 'read_file',
        'data': {
          'worktree': original['worktree'],
          'path': 'unrestricted-count.txt',
        },
      }),
    );
    final untouched = jsonDecode(await connection.execute(request: before));
    check(
      index == 0
          ? (untouched['Err']?['code'] == 'not_found')
          : (untouched['Ok']?['data']?['text'] ==
                List.filled(index, 'x').join()),
      'pending unrestricted commands have no side effects',
    );
    final remember = await connection.prepare(
      command: jsonEncode({
        'kind': 'resolve_approval',
        'data': {
          'session': session,
          'approval': command['id'],
          'decision': 'approve_session',
        },
      }),
    );
    check(
      jsonDecode(await connection.execute(request: remember))['Err']?['code'] ==
          'invalid_request',
      'unrestricted consent cannot be remembered',
    );
    final resumed = await reopen();
    check(
      pending(resumed)['id'] == command['id'] &&
          pending(resumed)['state'] == 'pending',
      'current permission and reconnect do not approve the frozen command',
    );
    final allow = await connection.prepare(
      command: jsonEncode({
        'kind': 'resolve_approval',
        'data': {
          'session': session,
          'approval': command['id'],
          'decision': 'approve',
        },
      }),
    );
    final receipt = await connection.execute(request: allow);
    check(
      jsonDecode(receipt)['Ok']['data']['state'] == 'approved' &&
          jsonDecode(receipt)['Ok']['data']['choice'] == 'once',
      'explicit command decision is durable for this call',
    );
    check(
      await connection.execute(request: allow) == receipt,
      'decision retry reuses original authority',
    );
    final completed = await until(
      updates,
      (snapshot) => (snapshot['page']['runs'] as List).any(
        (run) => run['turn'] == turn['id'] && run['status'] == 'completed',
      ),
    );
    final page = completed['snapshot']['page'];
    final sources = (page['approvals'] as List)
        .map((approval) => approval['source'])
        .toList();
    check(
      jsonEncode(sources) ==
          jsonEncode(
            mode == 'ask'
                ? ['user', 'sandbox', 'user']
                : mode == 'project'
                ? ['project', 'sandbox', 'user']
                : ['full', 'sandbox', 'user'],
          ),
      'automatic authority retains its frozen source',
    );
    check(
      page['runs'].single['revision'] == config['revision'],
      'execution uses admitted revision',
    );
    check(
      await connection.execute(request: queue) == queued,
      'queued retry does not execute again',
    );
    final read = (await execute(connection, 'read_file', {
      'worktree': original['worktree'],
      'path': 'permission.txt',
    }))['data'];
    check(
      read['text'] == '自动与手动 中文 🙂',
      'actual write crosses shared file service',
    );
    final count = (await execute(connection, 'read_file', {
      'worktree': original['worktree'],
      'path': 'count.txt',
    }))['data'];
    check(
      count['text'] == List.filled(index + 1, 'x').join(),
      'each explicit session executes once',
    );
    check(
      jsonDecode(
            await connection.execute(request: before),
          )['Ok']['data']['text'] ==
          List.filled(index + 1, 'x').join(),
      'each approved unrestricted call executes once',
    );
    final replayed = await reopen();
    check(
      jsonEncode(replayed['snapshot']['page']) == jsonEncode(page),
      'reopening does not replay tools',
    );
    // Preserve fixture output so the next session can create a fresh file.
    final root = '${env['SAILRY_PROJECT_PATH']!}/project';
    await File('$root/permission.txt').rename('$root/permission-$mode.txt');
  }
  await close();
  RustLib.dispose();
  print('Dart FFI frozen permissions, automatic authority and retries passed');
}
