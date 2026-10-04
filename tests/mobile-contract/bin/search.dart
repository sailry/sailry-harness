import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'paging.dart' show complete;

Future<void> rejects(Future<dynamic> Function() action, String message) async {
  var rejected = false;
  try {
    await action().timeout(const Duration(seconds: 2));
  } catch (error) {
    rejected = error.toString().contains(message);
  }
  check(rejected, 'bridge rejects invalid or closed requests');
}

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final sessions = jsonDecode(env['SAILRY_SESSIONS']!) as List;
  final session = sessions[0] as String;
  final expected = jsonDecode(env['SAILRY_SEARCH_MATCH']!);
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  var updates = await connection.watchConversation(session: session);

  Future<Map<String, dynamic>> query(
    String text, {
    int? before,
    bool sensitive = false,
    int limit = 7,
    String? target,
    String? fault,
  }) async {
    final result = jsonDecode(
      await connection.searchConversation(
        session: target ?? session,
        query: jsonEncode({
          'text': text,
          'case_sensitive': sensitive,
          'before': before,
          'limit': limit,
        }),
      ),
    );
    if (fault != null) {
      check(result['Err']?['code'] == fault, 'typed search fault crosses FFI');
      return result;
    }
    check(result['Ok'] != null, 'shared Client search succeeds');
    check(
      result['Ok']['session'] == (target ?? session),
      'results retain session ownership',
    );
    return result['Ok'];
  }

  final recent = await until(updates, (_) => true);
  complete(recent);
  check(
    (recent['snapshot']['page']['runs'] as List).length == 20,
    'new watcher starts with recent turns',
  );
  check(
    (recent['snapshot']['page']['runs'] as List).every(
      (run) => run['turn'] != expected['turn'],
    ),
    'search target is not loaded',
  );
  await rejects(
    () => updates.loadThrough(sequence: BigInt.zero),
    'must be positive',
  );
  final found = (await query('äbc 中文 🙂'))['matches'] as List;
  check(
    found.length == 1 && jsonEncode(found.single) == jsonEncode(expected),
    'search returns canonical match',
  );
  final match = found.single;
  final bytes = utf8.encode(match['snippet']);
  check(
    utf8.decode(
          bytes.sublist(match['highlight']['start'], match['highlight']['end']),
        ) ==
        'ÄBC 中文 🙂',
    'highlight is an original UTF-8 byte range, not a Dart character offset',
  );
  check(
    ((await query('äbc', sensitive: true))['matches'] as List).isEmpty,
    'case-sensitive query differs',
  );
  check(
    ((await query('.*'))['matches'] as List).single['entry'] == match['entry'],
    'query is literal, not regex',
  );
  check(
    ((await query('absent'))['matches'] as List).isEmpty,
    'no match is not an error',
  );
  await query('', fault: 'invalid_request');
  await query('🙂' * 129, fault: 'invalid_request');
  await query('History', limit: 0, fault: 'invalid_request');
  await query(
    'History',
    before: match['sequence'],
    target: sessions[1],
    fault: 'wrong_target',
  );

  var page = await query('History');
  check(
    (page['matches'] as List).length == 7,
    'logical result page respects requested size',
  );
  final matches = List<dynamic>.from(page['matches']);
  final inserted = (await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': 1,
    'message': {'text': 'History inserted during search', 'attachments': []},
  }))['data'];
  await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == inserted['id'] && run['status'] == 'completed',
    ),
  );
  while (page['next_before'] != null) {
    page = await query('History', before: page['next_before']);
    matches.addAll(page['matches']);
  }
  check(
    matches.length == 44 &&
        matches.map((item) => item['entry']).toSet().length == 44,
    'continuation returns each original result once',
  );
  check(
    matches.every((item) => item['turn'] != inserted['id']),
    'old results do not mix in new turns',
  );
  for (var index = 1; index < matches.length; index++) {
    check(
      matches[index - 1]['sequence'] > matches[index]['sequence'],
      'result order is stable',
    );
  }
  check(
    (await query('History'))['matches'][0]['turn'] == inserted['id'],
    'fresh query sees new messages',
  );

  final configured = (await execute(connection, 'set_session_config', {
    'session': session,
    'expected_revision': 1,
    'config': jsonDecode(env['SAILRY_SLOW_CONFIG']!),
  }))['data'];
  final turn = (await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': configured['revision'],
    'message': {'text': 'Stream while locating 中文 🙂', 'attachments': []},
  }))['data'];
  await until(updates, (snapshot) => (snapshot['drafts'] as List).isNotEmpty);
  check(
    ((await query('partial-fixture'))['matches'] as List).isEmpty,
    'temporary model output is excluded',
  );

  Future<Map<String, dynamic>> locate() async {
    // A pending next call must not block a shared-reference FFI history request.
    final waiting = updates.next();
    await updates
        .loadThrough(sequence: BigInt.from(match['turn_sequence']))
        .timeout(const Duration(seconds: 2));
    var view =
        jsonDecode(await waiting.timeout(const Duration(seconds: 10)))
            as Map<String, dynamic>;
    complete(view);
    if ((view['snapshot']['page']['runs'] as List).length != 47 ||
        view['loading_older'] == true) {
      view = await until(
        updates,
        (snapshot) => (snapshot['page']['runs'] as List).length == 47,
      );
    }
    complete(view);
    check(view['loading_older'] == false, 'target loading has finished');
    check(
      view['snapshot']['page']['next_before'] == null,
      'first target loads the complete range',
    );
    final entry = (view['snapshot']['page']['entries'] as List).singleWhere(
      (entry) => entry['id'] == match['entry'],
    );
    check(
      entry['parts'][match['part']]['data'] ==
          'Marker ÄBC 中文 🙂 and literal .* [text]',
      'target resolves to complete original content',
    );
    return view;
  }

  final located = await locate();
  check(
    (located['snapshot']['drafts'] as List).single['parts'][0]['data'] ==
        'partial-fixture',
    'live stream survives search and loading',
  );
  await execute(connection, 'stop_turn', {'turn': turn['id']});
  final stopped = await until(
    updates,
    (snapshot) => snapshot['page']['runs'].last['status'] == 'cancelled',
  );
  check(
    (stopped['snapshot']['drafts'] as List).isEmpty,
    'stop clears only transient output',
  );
  final canonical = jsonEncode(stopped['snapshot']['page']);
  await updates.loadThrough(sequence: BigInt.from(match['turn_sequence']));
  final pending = updates.next().then((_) => true, onError: (_) => true);
  await updates.close();
  await pending.timeout(const Duration(seconds: 2));
  await rejects(
    () => updates.loadThrough(sequence: BigInt.one),
    'subscription is closed',
  );
  updates.dispose();
  await connection.close();
  await rejects(() => query('History'), 'connection is closed');
  connection.dispose();
  await controller.close();
  controller.dispose();

  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  updates = await connection.watchConversation(session: session);
  final reopened = await until(updates, (_) => true);
  check(
    (reopened['snapshot']['page']['runs'] as List).length == 20,
    'reopened controller starts recent',
  );
  check(
    jsonEncode((await query('äbc 中文 🙂'))['matches']) == jsonEncode(found),
    'search identity survives controller restart',
  );
  final restored = await locate();
  check(
    jsonEncode(restored['snapshot']['page']) == canonical,
    'same Client restores identical complete history',
  );
  await controller.close();
  await rejects(
    () => updates.loadThrough(sequence: BigInt.one),
    'subscription is closed',
  );
  await rejects(() => query('History'), 'connection is closed');
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  controller.dispose();
  RustLib.dispose();
  print(
    'Dart FFI search, Unicode ranges, stable cursors, target loading and lifecycle passed',
  );
}
