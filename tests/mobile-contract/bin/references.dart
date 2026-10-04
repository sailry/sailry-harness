import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;

List<dynamic> references(Map<String, dynamic> snapshot) =>
    (snapshot['page']['entries'] as List)
        .where((entry) => entry['author'] == 'user')
        .expand((entry) => entry['parts'] as List)
        .where((part) => part['kind'] == 'reference')
        .map((part) => part['data'])
        .toList();

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
  final session = env['SAILRY_SESSION']!;
  var updates = await connection.watchConversation(session: session);
  await until(updates, (_) => true);
  final selected = [
    {
      'target': {'kind': 'file', 'data': 'source.txt'},
      'label': 'source.txt',
    },
    {
      'target': {'kind': 'agent', 'data': jsonDecode(env['SAILRY_ROLE']!)},
      'label': '@review',
    },
  ];
  await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': 2,
    'message': {
      'text': 'Inspect the context',
      'attachments': [],
      'references': selected,
    },
  });
  final done = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['status'] == 'completed',
    ),
  );
  check(
    jsonEncode(references(done['snapshot'])) == jsonEncode(selected),
    'parent references persist',
  );
  final children = done['snapshot']['page']['children'] as List;
  check(children.length == 1, 'selected role executes exactly one child');
  final childUpdates = await connection.watchConversation(
    session: children.single['run']['session'],
  );
  final child = await until(
    childUpdates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['status'] == 'completed',
    ),
  );
  check(
    jsonEncode(references(child['snapshot'])) == jsonEncode([selected.first]),
    'child receives context without recursive role selection',
  );
  await childUpdates.close();
  childUpdates.dispose();
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
  check(
    jsonEncode(references(restored['snapshot'])) == jsonEncode(selected),
    'controller reopening restores tags',
  );
  check(
    jsonEncode(restored['snapshot']['page']) ==
        jsonEncode(done['snapshot']['page']),
    'reopening does not dispatch again',
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
}
