import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;

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
  var updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  final queued = (await execute(connection, 'queue_turn', {
    'session': session,
    'expected_revision': 1,
    'message': {'text': 'Find the source', 'attachments': []},
  }))['data'];
  final provider = jsonDecode(env['SAILRY_PROVIDER']!);
  final gemini = provider['api'] == 'gemini';
  provider['models'][0]['web_search'] = false;
  await execute(connection, 'put_provider', {
    'provider': provider,
    'expected_revision': provider['revision'],
  });
  final start = await connection.prepare(
    command: jsonEncode({
      'kind': 'start_queued_turn',
      'data': {'turn': queued['id']},
    }),
  );
  final receipt = await connection.execute(request: start);
  check(jsonDecode(receipt)['Ok'] != null, 'queued search is admitted');
  final view = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == queued['id'] && run['status'] == 'completed',
    ),
  );
  final entry = (view['snapshot']['page']['entries'] as List).singleWhere(
    (entry) => (entry['citations'] as List).isNotEmpty,
  );
  final citations = entry['citations'] as List;
  if (gemini) {
    check(
      citations.length == 1 &&
          citations[0]['uri'] == 'https://example.com/source',
      'grounding sources cross FFI',
    );
    check(
      (entry['search_suggestions'] as String).contains('<style>') &&
          (entry['search_suggestions'] as String).contains(
            'https://www.google.com/search?q=fixture',
          ),
      'provider HTML and links remain unchanged',
    );
    check(
      !jsonEncode(entry).contains('sdkBlob'),
      'native SDK data stays on the Node',
    );
  } else {
    check(
      !jsonEncode(view).contains('encrypted-search-result-fixture') &&
          !jsonEncode(view).contains('encrypted-citation-index-fixture'),
      'native encrypted context stays on the execution Node',
    );
    check(
      citations.length == 2 &&
          citations[0]['start'] == 0 &&
          citations[0]['end'] == 6 &&
          citations[1]['start'] == 6,
      'citation ranges count Unicode scalars across parts',
    );
    check(
      citations.every(
        (citation) =>
            citation['uri'] == 'https://example.com/source_(one)' &&
            citation['title'] == 'Fixture source',
      ),
      'source titles and URLs cross FFI',
    );
    check(
      view['calls'][0]['name'] == 'web_search' &&
          view['calls'][0]['state'] == 'returned',
      'hosted tool completion uses shared Client',
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
  updates = await connection.watchConversation(session: session);
  final restored = await until(updates, (_) => true);
  final restoredEntry = (restored['snapshot']['page']['entries'] as List)
      .singleWhere((value) => value['id'] == entry['id']);
  check(
    jsonEncode(restoredEntry['citations']) == jsonEncode(citations),
    'reopened controller retains sources',
  );
  check(
    restoredEntry['search_suggestions'] == entry['search_suggestions'],
    'reopened controller retains search suggestions',
  );
  check(
    await connection.execute(request: start) == receipt,
    'original start receipt does not replay search',
  );
  final next = (await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': 1,
    'message': {'text': 'Continue without search', 'attachments': []},
  }))['data'];
  final completed = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == next['id'] && run['status'] == 'completed',
    ),
  );
  check(
    (completed['snapshot']['page']['entries'] as List)
        .where((entry) => entry['turn'] == next['id'])
        .every((entry) => (entry['citations'] as List).isEmpty),
    'new turn uses changed search settings',
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  stdout.writeln('Native web search contract passed');
}
