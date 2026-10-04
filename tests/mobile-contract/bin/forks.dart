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
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  var sourceUpdates = await connection.watchConversation(session: source);
  await until(sourceUpdates, (_) => true);

  Future<Map<String, dynamic>> history(String session) async => (await execute(
    connection,
    'read_conversation',
    {'session': session, 'before': null, 'limit': 100},
  ))['data']['page'];
  final baseline = await history(source);
  final first = (baseline['runs'] as List).first;
  final last = (baseline['runs'] as List).last;
  final other = (await history(sessions[1]))['runs'][0]['turn'];
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
  await until(
    sourceUpdates,
    (snapshot) => (snapshot['drafts'] as List).isNotEmpty,
  );
  final current = (await execute(connection, 'set_session_config', {
    'session': source,
    'expected_revision': slow['revision'],
    'config': original['config'],
  }))['data'];
  final queue = (await history(source))['queue'];
  await execute(connection, 'set_queue_paused', {
    'session': source,
    'expected_revision': queue['revision'],
    'paused': true,
  });
  await execute(connection, 'queue_turn', {
    'session': source,
    'expected_revision': current['revision'],
    'message': {'text': 'Never inherited pending input', 'attachments': []},
  });

  Future<String> prepare(String target, String through, int revision) =>
      connection.prepare(
        command: jsonEncode({
          'kind': 'fork_conversation',
          'data': {
            'session': target,
            'through': through,
            'expected_revision': revision,
          },
        }),
      );
  for (final invalid in [
    [last['turn'], 1, 'revision_conflict'],
    [active['id'], current['revision'], 'busy'],
    [other, current['revision'], 'wrong_target'],
  ]) {
    final request = await prepare(
      source,
      invalid[0] as String,
      invalid[1] as int,
    );
    final result = jsonDecode(await connection.execute(request: request));
    check(
      result['Err']?['code'] == invalid[2],
      'fork target and revision checks cross FFI',
    );
  }
  final request = await prepare(source, last['turn'], current['revision']);
  // Drop the application result, then recover the same accepted command.
  await connection.execute(request: request);
  final recovered = await connection.execute(request: request);
  final branch = jsonDecode(recovered)['Ok']['data'];
  check(
    branch['id'] != source && branch['revision'] == 1,
    'branch is an independent session',
  );
  check(
    branch['fork']['session'] == source &&
        branch['fork']['through'] == last['turn'],
    'fork provenance is explicit',
  );
  check(
    jsonEncode(branch['config']) == jsonEncode(current['config']),
    'branch uses the current source configuration',
  );
  final altered = jsonDecode(request);
  altered['command']['data']['through'] = first['turn'];
  check(
    jsonDecode(
          await connection.execute(request: jsonEncode(altered)),
        )['Err']?['code'] ==
        'conflict',
    'request content cannot change',
  );

  var updates = await connection.watchConversation(session: branch['id']);
  final recent = await until(updates, (_) => true);
  complete(recent);
  check(
    (recent['snapshot']['page']['runs'] as List).length == 20,
    'branch retains logical turn paging',
  );
  await updates.loadOlder();
  final all = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).length == 24,
  );
  complete(all);
  final page = all['snapshot']['page'];
  check(
    jsonEncode(page['entries']) == jsonEncode(baseline['entries']),
    'inherited native entries retain identity and content',
  );
  check(
    (page['runs'] as List).every(
      (run) =>
          run['session'] == branch['id'] &&
          run['origin'] == source &&
          run['revision'] == 1,
    ),
    'inherited revisions retain their original owner',
  );
  check(
    (page['queue']['items'] as List).isEmpty &&
        page['queue']['paused'] == false,
    'source queue is not inherited',
  );
  check(
    (all['calls'] as List).length == 1,
    'shared projection preserves the inherited tool',
  );
  final matches =
      jsonDecode(
            await connection.searchConversation(
              session: branch['id'],
              query: jsonEncode({
                'text': 'Fork marker 中文 🙂',
                'case_sensitive': true,
                'before': null,
                'limit': 7,
              }),
            ),
          )['Ok']['matches']
          as List;
  check(
    matches.single['turn'] == first['turn'],
    'branch search retains the original turn',
  );

  await execute(connection, 'stop_turn', {'turn': active['id']});
  await until(
    sourceUpdates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == active['id'] && run['status'] == 'cancelled',
    ),
  );
  await sourceUpdates.close();
  sourceUpdates.dispose();
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
    'controller reopen keeps the same fork result',
  );
  updates = await connection.watchConversation(session: branch['id']);
  await until(updates, (_) => true);
  await updates.loadThrough(sequence: BigInt.from(first['sequence']));
  final restored = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).length == 24,
  );
  complete(restored);
  check(
    jsonEncode(restored['snapshot']['page']) == jsonEncode(page),
    'shared history recovers after controller reopen',
  );
  final sent = (await execute(connection, 'submit_turn', {
    'session': branch['id'],
    'expected_revision': 1,
    'message': {'text': 'Mobile branch continuation 中文 🙂', 'attachments': []},
  }))['data'];
  final continued = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == sent['id'] && run['status'] == 'completed',
    ),
  );
  complete(continued);
  final own = (continued['snapshot']['page']['runs'] as List).last;
  check(
    own['origin'] == null && own['session'] == branch['id'],
    'new execution belongs to the branch',
  );
  check(
    (continued['calls'] as List).length == 1,
    'inherited tool is not reexecuted',
  );
  final nested = (await execute(connection, 'fork_conversation', {
    'session': branch['id'],
    'through': first['turn'],
    'expected_revision': 1,
  }))['data'];
  final nestedUpdates = await connection.watchConversation(
    session: nested['id'],
  );
  final nestedView = await until(nestedUpdates, (_) => true);
  complete(nestedView);
  check(
    (nestedView['snapshot']['page']['runs'] as List).single['origin'] == source,
    'nested forks retain the canonical owner',
  );
  check(
    jsonEncode(nestedView['snapshot']['page']['entries']) ==
            jsonEncode(
              (baseline['entries'] as List)
                  .where((entry) => entry['turn'] == first['turn'])
                  .toList(),
            ) &&
        (nestedView['calls'] as List).length == 1,
    'nested prefix remains a complete native tool turn',
  );
  final unchanged = await history(source);
  final inheritedTurns = (baseline['runs'] as List)
      .map((run) => run['turn'])
      .toSet();
  check(
    jsonEncode(
          (unchanged['entries'] as List)
              .where((entry) => inheritedTurns.contains(entry['turn']))
              .toList(),
        ) ==
        jsonEncode(baseline['entries']),
    'branch continuation does not change source history',
  );
  check(
    (unchanged['queue']['items'] as List).length == 1,
    'source pending input is retained',
  );
  await nestedUpdates.close();
  nestedUpdates.dispose();
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  stdout.writeln(
    'Dart FFI fork provenance, paging, retries and independent resume passed',
  );
}
