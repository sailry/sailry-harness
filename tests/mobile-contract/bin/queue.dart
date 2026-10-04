import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'attachments/support.dart' show uploadAttachment;

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
  final attachment = (await uploadAttachment(
    connection,
    env['SAILRY_WORKTREE']!,
    Uint8List.fromList(utf8.encode('Complete FFI attachment input')),
    name: 'input.txt',
    mediaType: 'text/plain',
  ))['id'];
  var updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  await execute(connection, 'set_queue_paused', {
    'session': session,
    'expected_revision': 0,
    'paused': true,
  });
  final turns = <String>[];
  for (final message in ['first', 'removed', 'third']) {
    final turn = (await execute(connection, 'queue_turn', {
      'session': session,
      'expected_revision': 1,
      'message': {
        'text': message,
        'attachments': message == 'first' ? [attachment] : [],
      },
    }))['data'];
    turns.add(turn['id']);
    await execute(connection, 'start_queued_turn', {'turn': turn['id']});
  }
  final completeInput = List.filled(100, '完整内容 🙂\n').join();
  final edit = await connection.prepare(
    command: jsonEncode({
      'kind': 'edit_queued_turn',
      'data': {
        'turn': turns[0],
        'expected_revision': 1,
        'message': {
          'text': completeInput,
          'attachments': [attachment],
        },
      },
    }),
  );
  final edited = await connection.execute(request: edit);
  check(jsonDecode(edited)['Ok']?['kind'] == 'queue', 'edit admitted over FFI');
  final view = await until(
    updates,
    (snapshot) => (snapshot['page']['queue']['items'] as List).any(
      (item) => item['revision'] == 2,
    ),
  );
  final queue = view['snapshot']['page']['queue'];
  final first = (queue['items'] as List).first;
  check(
    first['attachments'][0]['id'] == attachment &&
        first['attachments'][0]['spec']['name'] == 'input.txt',
    'queue retains attachment metadata',
  );
  check(
    first['truncated'] && (first['preview'] as String).runes.length == 256,
    'bounded preview crosses FFI',
  );
  check(
    first['config_revision'] == 1,
    'sent configuration revision is unchanged',
  );
  check(
    view['snapshot']['page']['entries'].isEmpty,
    'queued edits do not create execution history',
  );
  final full = await execute(connection, 'read_queued_turn', {
    'turn': turns[0],
  });
  check(
    full['data']['message']['text'] == completeInput,
    'editor reads full Unicode message',
  );
  check(
    full['data']['message']['attachments'][0] == attachment,
    'editor retains attachment references',
  );
  await execute(connection, 'move_queued_turn', {
    'session': session,
    'expected_revision': queue['revision'],
    'turn': turns[2],
    'before': turns[0],
  });
  await execute(connection, 'remove_queued_turn', {
    'turn': turns[1],
    'expected_revision': 1,
  });
  final stale = await connection.prepare(
    command: jsonEncode({
      'kind': 'edit_queued_turn',
      'data': {
        'turn': turns[0],
        'expected_revision': 1,
        'message': {'text': 'stale edit', 'attachments': []},
      },
    }),
  );
  check(
    jsonDecode(await connection.execute(request: stale))['Err']?['code'] ==
        'revision_conflict',
    'stale editor receives conflict',
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
  final pending = restored['snapshot']['page']['queue'];
  check(
    pending['items'][1]['attachments'][0]['id'] == attachment,
    'attachment survives controller reopen',
  );
  check(pending['paused'], 'controller close does not resume the queue');
  check(
    jsonEncode(
          (pending['items'] as List).map((item) => item['turn']).toList(),
        ) ==
        jsonEncode([turns[2], turns[0]]),
    'order and removal survive controller reopen',
  );
  check(
    await connection.execute(request: edit) == edited,
    'same request retains its original result',
  );
  await execute(connection, 'set_queue_paused', {
    'session': session,
    'expected_revision': pending['revision'],
    'paused': false,
  });
  final finished = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List)
            .where((run) => run['status'] == 'completed')
            .length ==
        2,
  );
  final page = finished['snapshot']['page'];
  check(page['queue']['items'].isEmpty, 'completed queue is empty');
  final messages = (page['entries'] as List)
      .where((entry) => entry['author'] == 'user')
      .toList();
  check(
    messages.length == 2 &&
        messages[0]['parts'][0]['data'] == 'third' &&
        messages[1]['parts'][0]['data'] == completeInput,
    'ADK executes edited messages in shared queue order',
  );
  check(
    messages[1]['parts'][1]['kind'] == 'attachment' &&
        messages[1]['parts'][1]['data']['id'] == attachment,
    'canonical history projects the attachment',
  );
  final held = (await execute(connection, 'queue_turn', {
    'session': session,
    'expected_revision': 1,
    'message': {'text': 'send now', 'attachments': []},
  }))['data'];
  final sendNow = await connection.prepare(
    command: jsonEncode({
      'kind': 'send_queued_turn',
      'data': {'turn': held['id'], 'expected_revision': 1},
    }),
  );
  final sent = await connection.execute(request: sendNow);
  check(
    jsonDecode(sent)['Ok']?['kind'] == 'queue',
    'held input is sent immediately',
  );
  await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == held['id'] && run['status'] == 'completed',
    ),
  );
  check(
    await connection.execute(request: sendNow) == sent,
    'immediate send is not repeated',
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI queue editing, ordering and resume passed');
}
