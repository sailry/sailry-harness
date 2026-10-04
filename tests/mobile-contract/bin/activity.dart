import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;

Future<Map<String, dynamic>> observed(
  Updates updates,
  bool Function(Map<String, dynamic>) ready,
) => Future(() async {
  while (true) {
    final view = jsonDecode(await updates.next()) as Map<String, dynamic>;
    if (view['connected'] == true && ready(view)) return view;
  }
}).timeout(const Duration(seconds: 10));

Map<String, dynamic> activity(Map<String, dynamic> view, String session) =>
    (view['snapshot']['sessions'] as List).singleWhere(
      (value) => value['id'] == session,
    )['activity'];

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
  final addresses = [
    await controller.pair(ticket: env['SAILRY_INVITATION']!),
    await controller.pair(ticket: env['SAILRY_SECOND_INVITATION']!),
  ];
  final sessions = List<String>.from(jsonDecode(env['SAILRY_SESSIONS']!));
  final connections = [
    for (final address in addresses) await controller.connect(address: address),
  ];
  final watches = [
    for (final connection in connections) await connection.watch(),
  ];
  for (final updates in watches) {
    await observed(updates, (_) => true);
  }
  check(
    jsonDecode(await controller.notifications())['unread'] == 0,
    'no historical notices',
  );
  for (var index = 0; index < 2; index++) {
    await execute(connections[index], 'submit_turn', {
      'session': sessions[index],
      'expected_revision': 1,
      'message': {'text': 'Write the requested file', 'attachments': []},
    });
    final view = await observed(
      watches[index],
      (view) => activity(view, sessions[index])['waiting'] == 'approval',
    );
    final summary = activity(view, sessions[index]);
    check(
      summary['run']['status'] == 'running' && summary['queued'] == 0,
      'shared activity retains the running task while waiting',
    );
  }
  var inbox = jsonDecode(await controller.notifications());
  final notices = inbox['notices'] as List;
  check(
    notices.length == 2 &&
        inbox['unread'] == 2 &&
        notices
                .map((notice) => jsonEncode(notice['id']['node']))
                .toSet()
                .length ==
            2,
    'controller keeps both hosts and their resource identities',
  );
  final first = notices.singleWhere(
    (notice) => notice['id']['target']['Session'] == sessions[0],
  );
  await controller.markNotificationRead(id: jsonEncode(first['id']));
  check(
    jsonDecode(await controller.notifications())['unread'] == 1,
    'mark one read',
  );
  final queued = (await execute(connections[0], 'submit_turn', {
    'session': sessions[0],
    'expected_revision': 1,
    'message': {'text': 'Queued next task', 'attachments': []},
  }))['data'];
  await observed(
    watches[0],
    (view) => activity(view, sessions[0])['queued'] == 1,
  );
  inbox = jsonDecode(await controller.notifications());
  check(
    inbox['notices'].length == 2 && inbox['unread'] == 1,
    'repeated feed delivery neither duplicates nor resets read state',
  );
  for (var index = 0; index < 2; index++) {
    final conversation = await connections[index].watchConversation(
      session: sessions[index],
    );
    final waiting = await until(
      conversation,
      (snapshot) => (snapshot['page']['approvals'] as List).any(
        (value) => value['state'] == 'pending',
      ),
    );
    final approval = (waiting['snapshot']['page']['approvals'] as List)
        .singleWhere((value) => value['state'] == 'pending');
    await execute(connections[index], 'resolve_approval', {
      'session': sessions[index],
      'approval': approval['id'],
      'decision': 'approve',
    });
    await observed(
      watches[index],
      (view) => activity(view, sessions[index])['waiting'] == 'input',
    );
    final asking = await until(
      conversation,
      (snapshot) => (snapshot['page']['questions'] as List).any(
        (value) => value['state']['kind'] == 'pending',
      ),
    );
    final question = (asking['snapshot']['page']['questions'] as List)
        .singleWhere((value) => value['state']['kind'] == 'pending');
    await execute(connections[index], 'resolve_question', {
      'session': sessions[index],
      'question': question['id'],
      'response': {
        'kind': 'answer',
        'data': {'kind': 'text', 'data': 'Continue'},
      },
    });
    final done = await observed(watches[index], (view) {
      final summary = activity(view, sessions[index]);
      return summary['run']['status'] == 'completed' &&
          (index != 0 || summary['run']['turn'] == queued['id']);
    });
    check(
      activity(done, sessions[index])['queued'] == 0,
      'queue drains on the execution Node',
    );
    await conversation.close();
    conversation.dispose();
  }
  inbox = jsonDecode(await controller.notifications());
  final completed = (inbox['notices'] as List).where(
    (notice) => notice['kind'] == 'Completed',
  );
  check(
    completed.length == 3 && inbox['notices'].length == 7,
    'fast consecutive completion and input notices survive coalescing',
  );
  await controller.markNotificationsRead();
  check(
    jsonDecode(await controller.notifications())['unread'] == 0,
    'mark all read',
  );
  await connections[0].close();
  check(
    jsonDecode(await controller.notifications())['notices'].length == 7,
    'closing a host view preserves the controller inbox',
  );
  for (final updates in watches) {
    await updates.close();
    updates.dispose();
  }
  for (final connection in connections) {
    connection.dispose();
  }
  await controller.close();
  var closed = false;
  try {
    await controller.notifications();
  } catch (_) {
    closed = true;
  }
  check(closed, 'closed controller rejects inbox access');
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  for (final address in addresses) {
    final connection = await controller.connect(address: address);
    final updates = await connection.watch();
    await observed(updates, (_) => true);
    await updates.close();
    updates.dispose();
    await connection.close();
    connection.dispose();
  }
  check(
    jsonDecode(await controller.notifications())['notices'].isEmpty,
    'reopening reads historical state without republishing completion',
  );
  await controller.close();
  controller.dispose();
  print('Dart activity, shared inbox, queue completion and read state passed');
}
