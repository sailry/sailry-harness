import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'approvals.dart' show part;
import 'conversation.dart' show canonicalJson, check, detail, execute, until;

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
  final clock = Stopwatch()..start();
  void phase(String name, [int? count]) => stderr.writeln(
    'Dart paging phase=$name elapsed_ms=${clock.elapsedMilliseconds}${count == null ? '' : ' count=$count'}',
  );

  phase('main');
  final env = Platform.environment;
  phase('bridge init start');
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  phase('bridge init returned');
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final sessions = jsonDecode(env['SAILRY_SESSIONS']!) as List;
  phase('controller open start');
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  phase('controller open returned');
  phase('pair start');
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  phase('pair returned');
  phase('connect start');
  var connection = await controller.connect(address: address);
  phase('connect returned');
  final session = sessions[0] as String;

  phase('large watch start');
  final large = await connection.watchConversation(session: sessions[1]);
  phase('large watch returned');
  phase('large hydrate start');
  final hydrated = await until(large, (snapshot) {
    check(
      (snapshot['missing'] as List).isEmpty,
      'initial snapshot is hydrated before publication',
    );
    return true;
  });
  phase('large hydrate returned', (hydrated['calls'] as List).length);
  complete(hydrated);
  messages(hydrated['snapshot']['page'], 1, 1, 'ffi-history');
  check(
    (hydrated['calls'] as List).length == 51 &&
        (hydrated['calls'] as List).every(
          (call) => call['state'] == 'returned',
        ),
    'large turn retains all 51 complete tool exchanges before publication',
  );
  phase('large close start');
  await large.close();
  phase('large close returned');
  await rejects(large.loadOlder);
  phase('large closed read rejected');
  large.dispose();

  phase('raw read start');
  final raw = (await execute(connection, 'read_conversation', {
    'session': sessions[1],
    'before': null,
    'limit': 1,
  }))['data'];
  phase('raw read returned', (raw['page']['entries'] as List).length);
  check(
    (raw['missing'] as List).length == 1 &&
        (raw['page']['entries'] as List).length == 100,
    'raw bounded transport is distinct from a complete logical page',
  );
  for (final (target, before, limit, code) in [
    (sessions[1], env['SAILRY_FIRST_TURN'], 20, 'wrong_target'),
    (session, null, 0, 'invalid_request'),
  ]) {
    phase('invalid read prepare start', limit);
    final request = await connection.prepare(
      command: jsonEncode({
        'kind': 'read_conversation',
        'data': {'session': target, 'before': before, 'limit': limit},
      }),
    );
    phase('invalid read prepare returned', limit);
    phase('invalid read execute start', limit);
    final result = jsonDecode(await connection.execute(request: request));
    phase('invalid read execute returned', limit);
    check(
      result['Err']?['code'] == code,
      'invalid cursor or page size is rejected',
    );
  }

  phase('recent watch start');
  var updates = await connection.watchConversation(session: session);
  phase('recent watch returned');
  phase('recent hydrate start');
  final recent = await until(updates, (_) => true);
  phase(
    'recent hydrate returned',
    (recent['snapshot']['page']['runs'] as List).length,
  );
  complete(recent);
  messages(recent['snapshot']['page'], 20, 20, 'ffi-history');
  check(
    (recent['snapshot']['page']['runs'] as List).length == 20,
    'initial history contains twenty complete turns',
  );

  phase('configuration start');
  final configured = (await execute(connection, 'set_session_config', {
    'session': session,
    'expected_revision': 1,
    'config': jsonDecode(env['SAILRY_SLOW_CONFIG']!),
  }))['data'];
  phase('configuration returned');
  phase('stream submit start');
  final turn = (await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': configured['revision'],
    'message': {'text': 'Stream while paging 中文 🙂', 'attachments': []},
  }))['data'];
  phase('stream submit returned');
  phase('draft wait start');
  await until(updates, (snapshot) => (snapshot['drafts'] as List).isNotEmpty);
  phase('draft wait returned');

  Future<Map<String, dynamic>> load(int count) async {
    // The pending read holds the receiver lock; paging must not wait for that lock.
    final waiting = updates.next();
    phase('older load start', count);
    await updates.loadOlder().timeout(const Duration(seconds: 2));
    phase('older load returned', count);
    phase('older update wait start', count);
    var view =
        jsonDecode(await waiting.timeout(const Duration(seconds: 10)))
            as Map<String, dynamic>;
    phase('older update wait returned', count);
    complete(view);
    if ((view['snapshot']['page']['runs'] as List).length != count ||
        view['loading_older'] == true) {
      phase('older count wait start', count);
      view = await until(
        updates,
        (snapshot) => (snapshot['page']['runs'] as List).length == count,
      );
      phase('older count wait returned', count);
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
  var details = 0;
  for (final call in all['calls'] as List) {
    phase('detail start', details + 1);
    final selected = await detail(updates, call as Map<String, dynamic>);
    phase('detail returned', ++details);
    final file = selected['result']['data'];
    check(
      call['state'] == 'returned' &&
          file['path'] == '资料.txt' &&
          file['offset'] == 0 &&
          file['next_offset'] == null &&
          jsonEncode(file['lines']) ==
              jsonEncode([
                {'number': 1, 'text': 'History 中文 🙂'},
              ]),
      'canonical tool results retain their complete Unicode content',
    );
  }
  phase('stop start');
  await execute(connection, 'stop_turn', {'turn': turn['id']});
  phase('stop returned');
  phase('cancel wait start');
  final stopped = await until(
    updates,
    (snapshot) => snapshot['page']['runs'].last['status'] == 'cancelled',
  );
  phase('cancel wait returned');
  check(
    (stopped['snapshot']['drafts'] as List).isEmpty,
    'stop clears transient text without replay',
  );
  final canonical = canonicalJson(stopped['snapshot']['page']);
  phase('exhausted load start');
  await updates.loadOlder();
  phase('exhausted load returned');
  final waiting = updates.next().then((_) => true, onError: (_) => true);
  phase('updates close start');
  await updates.close();
  phase('updates close returned');
  await waiting.timeout(const Duration(seconds: 2));
  phase('closed update wait returned');
  await rejects(updates.loadOlder);
  phase('closed older load rejected');
  updates.dispose();
  phase('connection close start');
  await connection.close();
  phase('connection close returned');
  connection.dispose();
  phase('controller close start');
  await controller.close();
  phase('controller close returned');
  controller.dispose();

  phase('controller reopen start');
  controller = await Controller.open(path: path, internet: false, relays: []);
  phase('controller reopen returned');
  phase('reconnect start');
  connection = await controller.connect(address: address);
  phase('reconnect returned');
  phase('reopened watch start');
  updates = await connection.watchConversation(session: session);
  phase('reopened watch returned');
  phase('reopened hydrate start');
  final reopened = await until(updates, (_) => true);
  phase('reopened hydrate returned');
  check(
    (reopened['snapshot']['page']['runs'] as List).length == 20,
    'new controller starts a new recent-history view',
  );
  await load(40);
  final restored = await load(46);
  check(
    canonicalJson(restored['snapshot']['page']) == canonical,
    'reopened FFI restores identical history without resubmitting turns',
  );
  phase('reopened controller close start');
  await controller.close();
  phase('reopened controller close returned');
  await rejects(updates.loadOlder);
  phase('reopened closed load rejected');
  await updates.close();
  phase('reopened updates close returned');
  updates.dispose();
  await connection.close();
  phase('reopened connection close returned');
  connection.dispose();
  controller.dispose();
  RustLib.dispose();
  phase('bridge disposed');
  print(
    'Dart FFI complete-turn paging, concurrent reads, live drafts and lifecycle passed',
  );
}
