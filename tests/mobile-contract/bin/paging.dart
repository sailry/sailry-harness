import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' show part;
import 'conversation.dart' show check, execute, until;

void complete(Map<String, dynamic> view) {
  check(view['older_error'] == null, 'older history read succeeds');
  final snapshot = view['snapshot'];
  check(
    (snapshot['missing'] as List).isEmpty,
    'only complete turns are published',
  );
  final page = snapshot['page'];
  final runs = page['runs'] as List;
  check(
    runs.map((run) => run['turn']).toSet().length == runs.length,
    'loaded turns are unique',
  );
  for (var index = 1; index < runs.length; index++) {
    check(
      runs[index - 1]['sequence'] < runs[index]['sequence'],
      'turn order is stable',
    );
  }
  final entries = page['entries'] as List;
  check(
    entries.map((entry) => entry['id']).toSet().length == entries.length,
    'canonical entries are unique',
  );
  final turns = runs.map((run) => run['turn']).toSet();
  check(
    entries.every((entry) => turns.contains(entry['turn'])),
    'every entry belongs to a loaded turn',
  );
  for (final call in view['calls'] as List) {
    final source = part(view, call['source']);
    check(source['id'] == call['id'], 'tool source retains its canonical ID');
    if (call['state'] == 'returned') {
      final response = part(view, call['response']);
      check(
        response['id'] == source['id'] && response['name'] == source['name'],
        'returned tool exchanges retain matching calls and results',
      );
    }
  }
}

void messages(Map<String, dynamic> page, int users, int answers, String model) {
  final entries = page['entries'] as List;
  check(
    entries.where((entry) => entry['author'] == 'user').length == users,
    'every admitted user message is retained exactly once',
  );
  check(
    entries
            .where((entry) => entry['author'] != 'user')
            .expand((entry) => entry['parts'] as List)
            .where(
              (part) =>
                  part['kind'] == 'text' && part['data'] == 'answer-$model',
            )
            .length ==
        answers,
    'every completed answer is retained exactly once',
  );
}

Future<void> rejects(Future<void> Function() action) async {
  var rejected = false;
  try {
    await action().timeout(const Duration(seconds: 2));
  } catch (error) {
    rejected = error.toString().contains('conversation subscription is closed');
  }
  check(rejected, 'closed subscription rejects paging');
}

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
  final session = sessions[0] as String;

  final large = await connection.watchConversation(session: sessions[1]);
  final hydrated = await until(large, (snapshot) {
    check(
      (snapshot['missing'] as List).isEmpty,
      'initial snapshot is hydrated before publication',
    );
    return true;
  });
  complete(hydrated);
  messages(hydrated['snapshot']['page'], 1, 1, 'ffi-history');
  check(
    (hydrated['calls'] as List).length == 51 &&
        (hydrated['calls'] as List).every(
          (call) => call['state'] == 'returned',
        ),
    'large turn retains all 51 complete tool exchanges before publication',
  );
  await large.close();
  await rejects(large.loadOlder);
  large.dispose();

  final raw = (await execute(connection, 'read_conversation', {
    'session': sessions[1],
    'before': null,
    'limit': 1,
  }))['data'];
  check(
    (raw['missing'] as List).length == 1 &&
        (raw['page']['entries'] as List).length == 100,
    'raw bounded transport is distinct from a complete logical page',
  );
  for (final (target, before, limit, code) in [
    (sessions[1], env['SAILRY_FIRST_TURN'], 20, 'wrong_target'),
    (session, null, 0, 'invalid_request'),
  ]) {
    final request = await connection.prepare(
      command: jsonEncode({
        'kind': 'read_conversation',
        'data': {'session': target, 'before': before, 'limit': limit},
      }),
    );
    final result = jsonDecode(await connection.execute(request: request));
    check(
      result['Err']?['code'] == code,
      'invalid cursor or page size is rejected',
    );
  }

  var updates = await connection.watchConversation(session: session);
  final recent = await until(updates, (_) => true);
  complete(recent);
  messages(recent['snapshot']['page'], 20, 20, 'ffi-history');
  check(
    (recent['snapshot']['page']['runs'] as List).length == 20,
    'initial history contains twenty complete turns',
  );

  final configured = (await execute(connection, 'set_session_config', {
    'session': session,
    'expected_revision': 1,
    'config': jsonDecode(env['SAILRY_SLOW_CONFIG']!),
  }))['data'];
  final turn = (await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': configured['revision'],
    'message': {'text': 'Stream while paging 中文 🙂', 'attachments': []},
  }))['data'];
  await until(updates, (snapshot) => (snapshot['drafts'] as List).isNotEmpty);

  Future<Map<String, dynamic>> load(int count) async {
    // The pending read holds the receiver lock; paging must not wait for that lock.
    final waiting = updates.next();
    await updates.loadOlder().timeout(const Duration(seconds: 2));
    var view =
        jsonDecode(await waiting.timeout(const Duration(seconds: 10)))
            as Map<String, dynamic>;
    complete(view);
    if ((view['snapshot']['page']['runs'] as List).length != count ||
        view['loading_older'] == true) {
      view = await until(
        updates,
        (snapshot) => (snapshot['page']['runs'] as List).length == count,
      );
    }
    complete(view);
    check(view['loading_older'] == false, 'paging completion is shared state');
    return view;
  }

  final older = await load(41);
  check(
    (older['snapshot']['drafts'] as List).single['parts'][0]['data'] ==
        'partial-fixture',
    'ongoing stream remains visible while loading old turns',
  );
  final all = await load(46);
  final page = all['snapshot']['page'];
  check(
    page['next_before'] == null &&
        page['runs'][0]['turn'] == env['SAILRY_FIRST_TURN'],
    'last page reaches the first admitted turn',
  );
  check(
    (all['calls'] as List).length == 51,
    'large older turn is hydrated and tools remain associated',
  );
  messages(page, 46, 45, 'ffi-history');
  check(
    (all['calls'] as List).every(
      (call) =>
          call['state'] == 'returned' &&
          part(all, call['response'])['result']['data']['text'] ==
              'History 中文 🙂',
    ),
    'canonical tool results retain their complete Unicode content',
  );
  await execute(connection, 'stop_turn', {'turn': turn['id']});
  final stopped = await until(
    updates,
    (snapshot) => snapshot['page']['runs'].last['status'] == 'cancelled',
  );
  check(
    (stopped['snapshot']['drafts'] as List).isEmpty,
    'stop clears transient text without replay',
  );
  final canonical = jsonEncode(stopped['snapshot']['page']);
  await updates.loadOlder();
  final waiting = updates.next().then((_) => true, onError: (_) => true);
  await updates.close();
  await waiting.timeout(const Duration(seconds: 2));
  await rejects(updates.loadOlder);
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();

  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  updates = await connection.watchConversation(session: session);
  final reopened = await until(updates, (_) => true);
  check(
    (reopened['snapshot']['page']['runs'] as List).length == 20,
    'new controller starts a new recent-history view',
  );
  await load(40);
  final restored = await load(46);
  check(
    jsonEncode(restored['snapshot']['page']) == canonical,
    'reopened FFI restores identical history without resubmitting turns',
  );
  await controller.close();
  await rejects(updates.loadOlder);
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  controller.dispose();
  RustLib.dispose();
  print(
    'Dart FFI complete-turn paging, concurrent reads, live drafts and lifecycle passed',
  );
}
