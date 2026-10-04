import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'attachments/support.dart'
    show uploadAttachment, prepareDownload, downloadAttachment;

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
  final session = (state['sessions'] as List).single;
  check(session['id'] == env['SAILRY_SESSION'], 'session retains its identity');
  check(
    session['config']['effort']['budget'] == 1024,
    'native budget crosses FFI',
  );
  check(
    session['profile']['provider']['api'] == env['SAILRY_NATIVE_API'],
    'native API survives source shutdown',
  );
  check(
    jsonEncode(session['config']['credential']['node']) ==
        jsonEncode(state['node']),
    'execution Node owns the credential',
  );
  final updates = await connection.watchConversation(session: session['id']);
  await until(updates, (_) => true);
  final files = <Map<String, dynamic>>[];
  for (final name in ['document.pdf', 'audio.wav', 'video.mp4']) {
    final bytes = await File.fromUri(
      Platform.script.resolve('../../fixtures/$name'),
    ).readAsBytes();
    final attachment = await uploadAttachment(
      connection,
      session['worktree'],
      bytes,
      name: '$name.bin',
      mediaType: 'application/octet-stream',
    );
    files.add({'bytes': bytes, 'attachment': attachment});
  }
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'submit_turn',
      'data': {
        'session': session['id'],
        'expected_revision': session['revision'],
        'message': {
          'text': 'Read native.txt and the files',
          'attachments': files.map((file) => file['attachment']['id']).toList(),
        },
      },
    }),
  );
  final result = await connection.execute(request: request);
  check(
    jsonDecode(result)['Ok']?['kind'] == 'queued_turn',
    'native turn admitted',
  );
  final complete = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['status'] == 'completed',
    ),
  );
  final page = complete['snapshot']['page'];
  final serialized = jsonEncode(page);
  for (final file in files) {
    check(
      serialized.contains(file['attachment']['id']) &&
          !serialized.contains(base64Encode(file['bytes'])),
      'history retains only the file reference',
    );
  }
  check(
    serialized.contains('原生响应 🙂') && serialized.contains('工具内容 🙂'),
    'native text and tool output cross FFI',
  );
  check(
    !serialized.contains('isolated-ffi-configuration-credential'),
    'credential stays out of history',
  );
  await updates.close();
  updates.dispose();
  final queued = (await execute(connection, 'queue_turn', {
    'session': session['id'],
    'expected_revision': session['revision'],
    'message': {'text': 'Continue the budget fixture', 'attachments': []},
  }))['data'];
  check(
    queued['config']['effort']['budget'] == 1024,
    'queued budget is explicit',
  );
  await execute(connection, 'set_session_config', {
    'session': session['id'],
    'expected_revision': session['revision'],
    'config': {...session['config'], 'effort': 'default'},
  });
  final beforeClose = await execute(connection, 'read_conversation', {
    'session': session['id'],
    'before': null,
    'limit': 100,
  });
  await controller.close();
  connection.dispose();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  check(
    await connection.execute(request: request) == result,
    'reconnect retains original admission',
  );
  final restored = await execute(connection, 'read_conversation', {
    'session': session['id'],
    'before': null,
    'limit': 100,
  });
  check(
    jsonEncode(restored['data']['page']) ==
        jsonEncode(beforeClose['data']['page']),
    'reopened controller restores canonical history',
  );
  for (final file in files) {
    check(
      base64Encode(
            await downloadAttachment(
              connection,
              await prepareDownload(connection, file['attachment']),
            ),
          ) ==
          base64Encode(file['bytes']),
      'file download survives controller restart',
    );
  }
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  check(
    snapshot['sessions'].single['config']['effort'] == 'default',
    'explicit default survives controller restart',
  );
  final resumed = await connection.watchConversation(session: session['id']);
  await until(resumed, (_) => true);
  await execute(connection, 'start_queued_turn', {'turn': queued['id']});
  await until(
    resumed,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == queued['id'] && run['status'] == 'completed',
    ),
  );
  await resumed.close();
  resumed.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI native provider resume passed');
}
