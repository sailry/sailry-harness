import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'paging.dart' show complete;

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final sessions = jsonDecode(env['SAILRY_SESSIONS']!) as List;
  final source = sessions[0] as String;
  final created = env['SAILRY_CREATED_FILE']!;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final session = (snapshot['sessions'] as List).singleWhere(
    (value) => value['id'] == source,
  );
  final worktree = session['worktree'];
  final config = (await execute(connection, 'set_session_config', {
    'session': source,
    'expected_revision': 1,
    'config': {...session['config'], 'permission': 'full'},
  }))['data'];
  var updates = await connection.watchConversation(session: source);
  await until(updates, (_) => true);
  final turn = (await execute(connection, 'submit_turn', {
    'session': source,
    'expected_revision': config['revision'],
    'message': {'text': 'Create file checkpoints 中文 🙂', 'attachments': []},
  }))['data']['id'];
  final completed = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == turn && run['status'] == 'completed',
    ),
  );
  complete(completed);
  final original = completed['snapshot']['page'];
  check(
    (completed['calls'] as List).length == 4,
    'every native write is projected',
  );

  Future<Map<String, dynamic>> page(
    String target,
    String? before,
    int limit,
  ) async => (await execute(connection, 'list_file_checkpoints', {
    'session': target,
    'turn': turn,
    'before': before,
    'limit': limit,
  }))['data'];
  Future<Map<String, dynamic>> content(String target, String id) async =>
      (await execute(connection, 'read_file_checkpoint', {
        'session': target,
        'checkpoint': id,
      }))['data'];
  Future<Map<String, dynamic>> read(String name) async => (await execute(
    connection,
    'read_file',
    {'worktree': worktree, 'path': name},
  ))['data'];
  Future<void> write(String name, String text) async {
    final current = await read(name);
    await execute(connection, 'write_file', {
      'worktree': worktree,
      'path': name,
      'text': text,
      'expected_revision': current['revision'],
    });
  }

  Future<String> restore(String target, Map<String, dynamic> file) =>
      connection.prepare(
        command: jsonEncode({
          'kind': 'restore_file_checkpoint',
          'data': {
            'session': target,
            'checkpoint': file['id'],
            'worktree': worktree,
          },
        }),
      );
  Future<void> rejects(
    String kind,
    Map<String, dynamic> data,
    String code,
  ) async {
    final request = await connection.prepare(
      command: jsonEncode({'kind': kind, 'data': data}),
    );
    check(
      jsonDecode(await connection.execute(request: request))['Err']?['code'] ==
          code,
      'checkpoint command rejects $code',
    );
  }

  Future<Map<String, dynamic>> history(String target) async => (await execute(
    connection,
    'read_conversation',
    {'session': target, 'before': null, 'limit': 100},
  ))['data']['page'];
  final first = await page(source, null, 2);
  check(
    first['session'] == source &&
        first['turn'] == turn &&
        (first['files'] as List).length == 2 &&
        first['next'] != null,
    'bounded first page identifies its owner',
  );
  final last = await page(source, first['next'], 2);
  check(
    last['next'] == null && (last['files'] as List).length == 2,
    'last page completes the turn',
  );
  final files = [
    ...first['files'],
    ...last['files'],
  ].cast<Map<String, dynamic>>();
  check(
    files.map((file) => file['id']).toSet().length == 4,
    'checkpoint IDs are unique across pages',
  );
  final fullBefore = List.filled(4000, '完整旧文件 中文 🙂\n').join();
  final fullAfter = List.filled(2000, '完整新文件 中文 🙂\n').join();
  for (final file in files) {
    check(
      file['turn'] == turn && file['worktree'] == worktree,
      'metadata preserves turn and worktree',
    );
    check(
      file['outcome']['kind'] == 'completed' &&
          file['outcome']['data']['Ok']['kind'] == 'file_written',
      'original durable receipt is visible',
    );
    final value = await content(source, file['id']);
    check(
      value['session'] == source &&
          jsonEncode(value['file']) == jsonEncode(file),
      'content retains exact checkpoint metadata',
    );
    final calls = (original['entries'] as List)
        .expand((entry) => entry['parts'] as List)
        .where((part) => part['kind'] == 'tool_call')
        .map((part) => part['data']);
    check(
      calls.any(
        (call) =>
            call['arguments']['path'] == file['path'] &&
            call['arguments']['text'] == value['after'],
      ),
      'after content is the canonical write proposal',
    );
    check(
      utf8.encode(value['after']).length == file['after']['size'],
      'full UTF-8 content crosses FFI',
    );
    if (file['path'] == created) {
      check(
        value['before'] == null && file['before'] == null,
        'original absence is distinct from empty text',
      );
    } else if (file['path'] == 'large.txt') {
      check(
        value['before'] == fullBefore && value['after'] == fullAfter,
        'large Unicode versions are exact and complete',
      );
    }
  }
  await rejects('list_file_checkpoints', {
    'session': source,
    'turn': turn,
    'before': null,
    'limit': 0,
  }, 'invalid_request');
  await rejects('list_file_checkpoints', {
    'session': source,
    'turn': turn,
    'before': sessions[1],
    'limit': 2,
  }, 'wrong_target');
  await rejects('read_file_checkpoint', {
    'session': sessions[1],
    'checkpoint': files[0]['id'],
  }, 'wrong_target');
  await rejects('restore_file_checkpoint', {
    'session': sessions[1],
    'checkpoint': files[0]['id'],
    'worktree': worktree,
  }, 'wrong_target');
  final newer = files.firstWhere((file) => file['path'] == 'source.txt');
  final older = files.lastWhere((file) => file['path'] == 'source.txt');
  final request = await restore(source, newer);
  // Discard application delivery, then recover only the same durable request.
  await connection.execute(request: request);
  check(
    (await read('source.txt'))['text'] == 'Middle source 中文 🙂',
    'restore writes the pre-change text',
  );
  await write('source.txt', 'Replacement survives retry');
  final recovered = await connection.execute(request: request);
  final restored = jsonDecode(recovered)['Ok'];
  check(
    restored['kind'] == 'file_restored' &&
        restored['data']['session'] == source &&
        restored['data']['checkpoint'] == newer['id'] &&
        restored['data']['revision'] == newer['before']['revision'],
    'typed restore receipt crosses FFI',
  );
  check(
    (await read('source.txt'))['text'] == 'Replacement survives retry',
    'same request does not overwrite a later edit',
  );
  await rejects('restore_file_checkpoint', {
    'session': source,
    'checkpoint': newer['id'],
    'worktree': worktree,
  }, 'revision_conflict');
  await write('source.txt', 'Final source 中文 🙂');
  check(
    jsonDecode(
          await connection.execute(request: await restore(source, newer)),
        )['Ok'] !=
        null,
    'explicit later restore rechecks the current file',
  );
  check(
    jsonDecode(
          await connection.execute(request: await restore(source, older)),
        )['Ok'] !=
        null,
    'sequential checkpoints restore in reverse order',
  );
  check(
    (await read('source.txt'))['text'] == 'Original source 中文 🙂',
    'original text is restored',
  );
  check(
    jsonEncode(await history(source)) == jsonEncode(original),
    'file restore leaves conversation history unchanged',
  );
  check(
    jsonEncode((await page(source, null, 100))['files']) == jsonEncode(files),
    'restore does not mutate original write receipts',
  );

  final branch = (await execute(connection, 'fork_conversation', {
    'session': source,
    'through': turn,
    'expected_revision': config['revision'],
  }))['data'];
  final waiting = updates.next();
  final backup = (await execute(connection, 'rewind_conversation', {
    'session': source,
    'through': null,
    'expected_head': turn,
    'expected_revision': original['revision'],
  }))['data']['backup'];
  final delivered = jsonDecode(await waiting) as Map<String, dynamic>;
  final rewound = delivered['snapshot']?['page']['revision'] == 2
      ? delivered
      : await until(updates, (snapshot) => snapshot['page']['revision'] == 2);
  complete(rewound);
  check(
    (rewound['snapshot']['page']['entries'] as List).isEmpty,
    'source projection follows the rewind',
  );
  await rejects('read_file_checkpoint', {
    'session': source,
    'checkpoint': newer['id'],
  }, 'wrong_target');
  for (final target in [branch['id'], backup['id']]) {
    check(
      jsonEncode((await page(target, null, 100))['files']) == jsonEncode(files),
      'branches reference the original checkpoints',
    );
  }
  final large = files.singleWhere((file) => file['path'] == 'large.txt');
  check(
    jsonDecode(
          await connection.execute(request: await restore(backup['id'], large)),
        )['Ok'] !=
        null,
    'backup can explicitly restore a visible checkpoint',
  );
  check(
    (await read('large.txt'))['text'] == fullBefore,
    'complete Unicode original is restored',
  );
  String? trashRequest;
  String? trashResult;
  if (env['SAILRY_NATIVE_TRASH'] == '1') {
    final absent = files.singleWhere((file) => file['path'] == created);
    trashRequest = await restore(branch['id'], absent);
    trashResult = await connection.execute(request: trashRequest);
    final result = jsonDecode(trashResult)['Ok']['data'];
    check(
      result['revision'] == null && result['path'] == created,
      'restoring absence returns a nullable file revision',
    );
    await rejects('read_file', {
      'worktree': worktree,
      'path': created,
    }, 'not_found');
    await execute(connection, 'write_file', {
      'worktree': worktree,
      'path': created,
      'text': 'Replacement survives retry',
      'expected_revision': null,
    });
    check(
      await connection.execute(request: trashRequest) == trashResult,
      'same absence receipt does not recycle a replacement',
    );
  }
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  check(
    await connection.execute(request: request) == recovered,
    'controller reopen retains the original caller and request receipt',
  );
  if (trashRequest != null) {
    check(
      await connection.execute(request: trashRequest) == trashResult,
      'absence receipt survives controller reopen',
    );
    check(
      (await read(created))['text'] == 'Replacement survives retry',
      'reopen does not recycle a replacement',
    );
  }
  updates = await connection.watchConversation(session: backup['id']);
  final resumed = await until(updates, (_) => true);
  complete(resumed);
  check(
    jsonEncode(resumed['snapshot']['page']['entries']) ==
        jsonEncode(original['entries']),
    'reopened backup retains every canonical tool event',
  );
  check(
    (await content(backup['id'], large['id']))['before'] == fullBefore,
    'checkpoint content survives controller reopen',
  );
  check(
    (await read('source.txt'))['text'] == 'Original source 中文 🙂',
    'receipt replay never reruns the earlier restore',
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  stdout.writeln(
    'Dart FFI checkpoint paging, restoration, backups and receipt recovery passed',
  );
}
