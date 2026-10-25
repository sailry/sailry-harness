import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'terminal.dart' as terminal;

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
  final create = await connection.prepare(
    command: jsonEncode({
      'kind': 'create_session',
      'data': {'project': null, 'worktree': null, 'config': null},
    }),
  );
  final created = await connection.execute(request: create);
  final session = jsonDecode(created)['Ok']['data'];
  check(
    session['project'] == null,
    'unassigned session retains Node ownership',
  );
  final worktree = session['worktree'];
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  check(
    (snapshot['projects'] as List).isEmpty &&
        (snapshot['worktrees'] as List).any((tree) => tree['id'] == worktree),
    'unassigned session has an execution worktree without a project',
  );
  final conversation = await connection.watchConversation(
    session: session['id'],
  );
  await until(conversation, (_) => true);
  final submit = await connection.prepare(
    command: jsonEncode({
      'kind': 'submit_turn',
      'data': {
        'session': session['id'],
        'expected_revision': session['revision'],
        'message': {'text': 'Unassigned FFI task', 'attachments': []},
      },
    }),
  );
  final admitted = await connection.execute(request: submit);
  check(
    jsonDecode(admitted)['Ok']['kind'] == 'queued_turn',
    'turn admitted over FFI',
  );
  await until(
    conversation,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['status'] == 'completed',
    ),
  );
  await conversation.close();
  conversation.dispose();

  final color = {'red': 240, 'green': 241, 'blue': 242};
  final viewport = {
    'columns': 80,
    'rows': 24,
    'pixel_width': 640,
    'pixel_height': 384,
  };
  final info = (await execute(connection, 'create_terminal', {
    'access': 'workspace',
    'worktree': worktree,
    'viewport': viewport,
    'appearance': {
      'foreground': color,
      'background': {'red': 20, 'green': 21, 'blue': 22},
      'palette': List.filled(16, color),
      'color_scheme': 'dark',
    },
  }))['data'];
  final updates = await connection.watchTerminal(terminal: info['id']);
  final transferred = await terminal.until(
    updates,
    (snapshot) => snapshot['info']['revision'] > info['revision'],
  );
  final stale = await connection.prepare(
    command: jsonEncode({
      'kind': 'resize_terminal',
      'data': {
        'terminal': info['id'],
        'revision': info['revision'],
        'viewport': viewport,
      },
    }),
  );
  final rejected = jsonDecode(await connection.execute(request: stale));
  check(
    rejected['Err']?['code'] == 'revision_conflict',
    'terminal transfer invalidates the previous controller revision',
  );
  final reclaimed = (await execute(connection, 'claim_terminal', {
    'terminal': info['id'],
    'expected_revision': transferred['info']['revision'],
  }))['data'];
  check(
    reclaimed['revision'] == info['revision'] + 2,
    'FFI reclaims exact control',
  );
  await execute(connection, 'resize_terminal', {
    'terminal': info['id'],
    'revision': reclaimed['revision'],
    'viewport': viewport,
  });
  await execute(connection, 'close_terminal', {
    'worktree': worktree,
    'terminal': info['id'],
  });
  await updates.close();
  updates.dispose();

  await controller.close();
  connection.dispose();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  check(
    await connection.execute(request: create) == created,
    'creation retry is stable',
  );
  check(
    await connection.execute(request: submit) == admitted,
    'turn retry is stable',
  );
  final restored = (await execute(connection, 'snapshot', null))['data'];
  check(
    (restored['sessions'] as List).length == 1,
    'recovery keeps one session',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI unassigned session and terminal ownership passed');
}
