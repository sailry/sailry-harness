import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'terminal.dart' as terminal;
import 'conversation.dart' as conversation;
import 'usage.dart' as usage;

void check(bool condition, String message) {
  if (!condition) throw StateError(message);
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
  check(
    (await controller.peers()).single == address,
    'paired address persisted',
  );
  var connection = await controller.connect(address: address);
  await controller.networkChanged();
  final hostRequest = await connection.prepare(
    command: jsonEncode({'kind': 'inspect_host'}),
  );
  final host = jsonDecode(await connection.execute(request: hostRequest));
  check(
    host['Ok']['kind'] == 'host_info',
    'execution host information over FFI',
  );
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'register_project',
      'data': {'name': 'Dart contract', 'path': env['SAILRY_PROJECT_PATH']!},
    }),
  );
  final first = await connection.execute(request: request);
  check(
    jsonDecode(first)['Ok']['kind'] == 'project',
    'real project registration',
  );
  check(
    await connection.execute(request: request) == first,
    'stable request deduplicated',
  );

  final wrong = jsonDecode(request) as Map<String, dynamic>;
  final snapshotRequest = await connection.prepare(
    command: jsonEncode({'kind': 'snapshot'}),
  );
  final snapshot = jsonDecode(
    await connection.execute(request: snapshotRequest),
  );
  final worktree = snapshot['Ok']['data']['worktrees'][0]['id'];
  final terminalId = await terminal.create(connection, worktree);
  final usageFixture = await usage.create(
    connection,
    snapshot['Ok']['data']['projects'][0]['id'],
  );
  final conversationFixture = await conversation.create(
    connection,
    snapshot['Ok']['data']['projects'][0]['id'],
    worktree,
  );
  await usage.complete(usageFixture);
  final saveRequest = await connection.prepare(
    command: jsonEncode({
      'kind': 'write_file',
      'data': {
        'worktree': worktree,
        'path': 'ffi.txt',
        'text': 'Shared file contract',
        'expected_revision': null,
      },
    }),
  );
  final saved = await connection.execute(request: saveRequest);
  check(
    jsonDecode(saved)['Ok']['kind'] == 'file_written',
    'file publication over FFI',
  );
  check(
    await connection.execute(request: saveRequest) == saved,
    'file publication deduplicated',
  );
  final readRequest = await connection.prepare(
    command: jsonEncode({
      'kind': 'read_file',
      'data': {'worktree': worktree, 'path': 'ffi.txt'},
    }),
  );
  final content = jsonDecode(await connection.execute(request: readRequest));
  check(
    content['Ok']['data']['text'] == 'Shared file contract',
    'file content over FFI',
  );

  wrong['target'] = List.filled(32, 0);
  final rejected = jsonDecode(
    await connection.execute(request: jsonEncode(wrong)),
  );
  check(
    rejected['Err']['code'] == 'wrong_target',
    'shared request error contract',
  );

  for (var index = 0; index < 3; index++) {
    final updates = await connection.watch();
    final view = jsonDecode(
      await updates.next().timeout(const Duration(seconds: 10)),
    );
    check(view['snapshot']['projects'].length == 1, 'shared Client snapshot');
    // Attach the error observer before cancellation to avoid unhandled Futures.
    final pending = updates.next().then((_) => false, onError: (_) => true);
    await updates.close();
    check(
      await pending.timeout(const Duration(seconds: 2)),
      'subscription cancellation wakes next',
    );
    updates.dispose();
  }

  await controller.close();
  var closedNetwork = false;
  try {
    await controller.networkChanged();
  } catch (_) {
    closedNetwork = true;
  }
  check(closedNetwork, 'closed controller rejects network callbacks');
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  check(
    (await controller.peers()).single == address,
    'reopen restores pairing',
  );
  // A closed controller invalidates existing connection handles.
  var closed = false;
  try {
    await connection.execute(request: request);
  } catch (_) {
    closed = true;
  }
  check(closed, 'old connection is closed');
  connection.dispose();
  connection = await controller.connect(address: address);
  await terminal.restore(connection, terminalId, worktree);
  await conversation.restore(connection, conversationFixture);
  await usage.restore(connection, usageFixture);
  check(
    await connection.execute(request: request) == first,
    'reopened Link uses persisted trust',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print(
    'Dart FFI pairing, requests, recovery and subscription disposal passed',
  );
}
