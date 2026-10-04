import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'paging.dart' show complete, rejects;

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final sessions = jsonDecode(env['SAILRY_SESSIONS']!) as List;
  final source = sessions[0] as String;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  var updates = await connection.watchConversation(session: source);
  final recent = await until(updates, (_) => true);
  complete(recent);
  check(
    (recent['snapshot']['page']['runs'] as List).length == 20,
    'initial logical page has twenty turns',
  );
  await updates.loadOlder();
  final loaded = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).length == 24,
  );
  complete(loaded);
  final baseline = loaded['snapshot']['page'];
  final first = baseline['runs'][0];
  final last = (baseline['runs'] as List).last;

  Future<Map<String, dynamic>> history(String session) async => (await execute(
    connection,
    'read_conversation',
    {'session': session, 'before': null, 'limit': 100},
  ))['data']['page'];
  Future<String> prepare(
    String target,
    String? through,
    String head,
    int revision,
  ) => connection.prepare(
    command: jsonEncode({
      'kind': 'rewind_conversation',
      'data': {
        'session': target,
        'through': through,
        'expected_head': head,
        'expected_revision': revision,
      },
    }),
  );
  Future<void> rejected(String request, String code) async {
    check(
      jsonDecode(await connection.execute(request: request))['Err']?['code'] ==
          code,
      'rewind rejects $code',
    );
  }

  final other = await history(sessions[1]);
  await execute(connection, 'set_queue_paused', {
    'session': sessions[1],
    'expected_revision': other['queue']['revision'],
    'paused': true,
  });
  final queued = (await execute(connection, 'queue_turn', {
    'session': sessions[1],
    'expected_revision': 1,
    'message': {'text': 'Pending input', 'attachments': []},
  }))['data'];
  await rejected(
    await prepare(sessions[1], null, other['runs'][0]['turn'], 1),
    'busy',
  );
  await execute(connection, 'remove_queued_turn', {
    'turn': queued['id'],
    'expected_revision': 1,
  });
  final original =
      ((await execute(connection, 'snapshot', null))['data']['sessions']
              as List)
          .singleWhere((session) => session['id'] == source);
  final slow = (await execute(connection, 'set_session_config', {
    'session': source,
    'expected_revision': 1,
    'config': jsonDecode(env['SAILRY_SLOW_CONFIG']!),
  }))['data'];
  final active = (await execute(connection, 'submit_turn', {
    'session': source,
    'expected_revision': slow['revision'],
    'message': {'text': 'Active source turn', 'attachments': []},
  }))['data'];
  await until(updates, (snapshot) => (snapshot['drafts'] as List).isNotEmpty);
  await rejected(await prepare(source, first['turn'], active['id'], 1), 'busy');
  final current = (await execute(connection, 'set_session_config', {
    'session': source,
    'expected_revision': slow['revision'],
    'config': original['config'],
  }))['data'];
  await execute(connection, 'stop_turn', {'turn': active['id']});
  final stopped = await until(
    updates,
    (snapshot) =>
        (snapshot['page']['runs'] as List).last['status'] == 'cancelled',
  );
  complete(stopped);
  final before = stopped['snapshot']['page'];
  check(
    (before['runs'] as List).length == 25,
    'loaded range survives a live turn',
  );
  for (final invalid in [
    [first['turn'], last['turn'], 1, 'revision_conflict'],
    [first['turn'], active['id'], 2, 'revision_conflict'],
    [other['runs'][0]['turn'], active['id'], 1, 'wrong_target'],
  ]) {
    await rejected(
      await prepare(
        source,
        invalid[0] as String,
        invalid[1] as String,
        invalid[2] as int,
      ),
      invalid[3] as String,
    );
  }
  final request = await prepare(source, first['turn'], active['id'], 1);
  // Leave next pending across the reset; command execution must not wait for it.
  final waiting = updates.next();
  await connection.execute(request: request);
  final recovered = await connection.execute(request: request);
  final result = jsonDecode(recovered)['Ok']['data'];
  final backup = result['backup'];
  check(
    result['revision'] == 2 && result['through'] == first['turn'],
    'structural revision is separate from configuration',
  );
  check(
    backup['fork']['session'] == source &&
        backup['fork']['through'] == active['id'],
    'backup retains the complete previous head',
  );
  check(
    jsonEncode(backup['config']) == jsonEncode(current['config']),
    'backup uses the effective source configuration',
  );
  var rewound =
      jsonDecode(await waiting.timeout(const Duration(seconds: 10)))
          as Map<String, dynamic>;
  if (rewound['snapshot']?['page']['revision'] != 2 ||
      rewound['connected'] != true) {
    rewound = await until(
      updates,
      (snapshot) => snapshot['page']['revision'] == 2,
    );
  }
  complete(rewound);
  final page = rewound['snapshot']['page'];
  check(
    (page['runs'] as List).single['turn'] == first['turn'],
    'reset removes the old loaded suffix',
  );
  check(
    jsonEncode(page['entries']) ==
        jsonEncode(
          (baseline['entries'] as List)
              .where((entry) => entry['turn'] == first['turn'])
              .toList(),
        ),
    'retained native tool turn stays complete',
  );
  check(
    (rewound['calls'] as List).length == 1 &&
        (rewound['snapshot']['drafts'] as List).isEmpty,
    'shared tool projection and drafts recover',
  );
  final altered = jsonDecode(request);
  altered['command']['data']['through'] = null;
  await rejected(jsonEncode(altered), 'conflict');
  for (final (revision, code) in [
    (1, 'revision_conflict'),
    (2, 'wrong_target'),
  ]) {
    final chunk = await connection.prepare(
      command: jsonEncode({
        'kind': 'read_turn',
        'data': {
          'session': source,
          'turn': last['turn'],
          'expected_revision': revision,
          'before': null,
          'limit': 100,
        },
      }),
    );
    await rejected(chunk, code);
  }
  Future<Map<String, dynamic>> search(String session) async => jsonDecode(
    await connection.searchConversation(
      session: session,
      query: jsonEncode({
        'text': 'Removed message',
        'case_sensitive': true,
        'before': null,
        'limit': 100,
      }),
    ),
  )['Ok'];
  final empty = await search(source);
  check(
    empty['revision'] == 2 && (empty['matches'] as List).isEmpty,
    'search cannot return removed messages',
  );
  final matches = await search(backup['id']);
  check(
    matches['revision'] == 1 && (matches['matches'] as List).length == 23,
    'backup retains searchable removed messages',
  );
  final backupUpdates = await connection.watchConversation(
    session: backup['id'],
  );
  await until(backupUpdates, (_) => true);
  await backupUpdates.loadThrough(sequence: BigInt.from(first['sequence']));
  final saved = await until(
    backupUpdates,
    (snapshot) => (snapshot['page']['runs'] as List).length == 25,
  );
  complete(saved);
  check(
    jsonEncode(saved['snapshot']['page']['entries']) ==
        jsonEncode(before['entries']),
    'backup keeps every canonical entry',
  );
  check(
    (saved['snapshot']['page']['runs'] as List).every(
      (run) => run['origin'] == source && run['session'] == backup['id'],
    ),
    'backup preserves configuration ownership',
  );
  check(
    (saved['calls'] as List).length == 1,
    'backup keeps the original tool result',
  );
  await backupUpdates.close();
  backupUpdates.dispose();
  await updates.close();
  await rejects(updates.loadOlder);
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  check(
    await connection.execute(request: request) == recovered,
    'controller reopen recovers the identical result',
  );
  updates = await connection.watchConversation(session: source);
  final restored = await until(updates, (_) => true);
  complete(restored);
  check(
    jsonEncode(restored['snapshot']['page']) == jsonEncode(page),
    'reopen retains the rewound history',
  );
  final resumedConfig =
      ((await execute(connection, 'snapshot', null))['data']['sessions']
              as List)
          .singleWhere((session) => session['id'] == source);
  check(
    jsonEncode(<String, dynamic>{...resumedConfig}..remove('activity')) ==
        jsonEncode(<String, dynamic>{...current}..remove('activity')),
    'rewind does not alter effective configuration',
  );
  check(
    resumedConfig['activity']['run']['turn'] == first['turn'] &&
        resumedConfig['activity']['run']['status'] == 'completed',
    'rewound activity follows the retained completed turn',
  );
  final sent = (await execute(connection, 'submit_turn', {
    'session': source,
    'expected_revision': current['revision'],
    'message': {'text': 'Mobile continuation 中文 🙂', 'attachments': []},
  }))['data'];
  final continued = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == sent['id'] && run['status'] == 'completed',
    ),
  );
  complete(continued);
  check(
    (continued['snapshot']['page']['runs'] as List).length == 2 &&
        continued['snapshot']['page']['revision'] == 2,
    'continuation appends only to the retained prefix',
  );
  check(
    jsonEncode(await history(backup['id'])) ==
        jsonEncode(saved['snapshot']['page']),
    'source continuation leaves backup unchanged',
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  stdout.writeln(
    'Dart FFI rewind, backup, revision and independent resume passed',
  );
}
