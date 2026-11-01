import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/status.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, detail, execute, until;

Map<String, dynamic>? contribution(Map<String, dynamic> view, String name) =>
    (view['entries'] as List)
        .cast<Map<String, dynamic>>()
        .where((entry) => entry['package']['name'] == name)
        .firstOrNull;

List<String> segments(Map<String, dynamic>? entry) =>
    ((entry?['state']['segments'] as List?) ?? [])
        .map((segment) => segment['label']['label'] as String)
        .toList();

Future<Map<String, dynamic>> statusView(
  StatusUpdates updates,
  bool Function(Map<String, dynamic>) ready,
) => Future(() async {
  while (true) {
    final view = jsonDecode(await updates.next()) as Map<String, dynamic>;
    check(view['error'] == null, 'status observer must remain available');
    check((view['failures'] as List).isEmpty, 'status exports must succeed');
    if (view['connected'] == true && ready(view)) return view;
  }
}).timeout(const Duration(seconds: 10));

Future<void> headlessStatus(
  Connection connection,
  Map<String, dynamic> session,
) async {
  final scope = {'worktree': session['worktree'], 'session': session['id']};
  check(scope['worktree'] != null, 'session status retains its worktree');
  final updates = await connection.watchStatus(scope: jsonEncode(scope));
  try {
    final initial = await statusView(
      updates,
      (view) => segments(contribution(view, 'git')).join(' ') == '+1 -0',
    );
    check(
      contribution(initial, 'goals')?['state']['visible'] == false,
      'an unused goal stays hidden without Desktop',
    );
    const description = 'Inspect status 中文 🙂';
    await execute(connection, 'submit_turn', {
      'session': session['id'],
      'expected_revision': session['revision'],
      'message': {'text': '/goal $description', 'attachments': []},
    });
    final active = await statusView(
      updates,
      (view) => contribution(view, 'goals')?['state']['visible'] == true,
    );
    final goal = contribution(active, 'goals')!;
    check(
      segments(goal).join(' ') == 'Goal Running' &&
          goal['state']['details'].single['value'] == description &&
          goal['state']['label']['locales']['zh-CN'] == '目标',
      'the Host supplies persisted goal labels and description',
    );
    final file = (await execute(connection, 'read_file', {
      'worktree': session['worktree'],
      'path': 'status.txt',
    }))['data'];
    await execute(connection, 'write_file', {
      'worktree': session['worktree'],
      'path': 'status.txt',
      'text': 'base\none\ntwo\n',
      'expected_revision': file['revision'],
    });
    final changed = await statusView(
      updates,
      (view) => segments(contribution(view, 'git')).join(' ') == '+2 -0',
    );
    check(
      contribution(changed, 'git')!['state']['details'].any(
        (detail) =>
            detail['label']['label'] == 'Added' && detail['value'] == '2',
      ),
      'a Node file write refreshes actual Git status details',
    );
    await execute(connection, 'submit_turn', {
      'session': session['id'],
      'expected_revision': session['revision'],
      'message': {'text': '/goal clear', 'attachments': []},
    });
    await statusView(
      updates,
      (view) => contribution(view, 'goals')?['state']['visible'] == false,
    );
  } finally {
    await updates.close();
    updates.dispose();
  }
}

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
  final initial = await until(updates, (_) => true);
  check(
    (initial['calls'] as List).isEmpty,
    'a session starts without goal calls',
  );
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final original = (snapshot['sessions'] as List).singleWhere(
    (item) => item['id'] == session,
  );
  Future<void> close() async {
    await updates.close();
    updates.dispose();
    await connection.close();
    connection.dispose();
    await controller.close();
    controller.dispose();
  }

  if (env['SAILRY_STATUS_ONLY'] == '1') {
    try {
      await headlessStatus(connection, original as Map<String, dynamic>);
    } finally {
      await close();
      RustLib.dispose();
    }
    print('Dart FFI headless Git and goal status passed');
    return;
  }
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'submit_turn',
      'data': {
        'session': session,
        'expected_revision': original['revision'],
        'message': {'text': '/goal Verify the result 中文 🙂', 'attachments': []},
      },
    }),
  );
  final receipt = await connection.execute(request: request);
  check(
    jsonDecode(receipt)['Ok']?['kind'] == 'queued_turn',
    'turn was admitted',
  );
  final first = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['status'] == 'completed',
    ),
  );
  final calls = first['calls'] as List;
  check(
    calls.length == 3,
    'the model reads the goal, verifies evidence and completes it',
  );
  final reader = calls.singleWhere(
    (call) => call['name'] == env['SAILRY_GET_GOAL'],
  );
  final writer = calls.singleWhere(
    (call) => call['name'] == env['SAILRY_UPDATE_GOAL'],
  );
  check(
    calls.singleWhere(
          (call) => call['name'] == env['SAILRY_READ_TOOL'],
        )['id'] !=
        null,
    'the evidence read retains its call identity',
  );
  final originalGoal = (await detail(connection, reader))['result']['goal'];
  final output = await detail(connection, writer);
  final goal = output['result']['goal'];
  check(
    originalGoal['state'] == 'active' &&
        originalGoal['revision'] == '1' &&
        goal['state'] == 'completed' &&
        goal['revision'] == '2' &&
        originalGoal['id'] == goal['id'],
    'package-owned revisions arrive through canonical results',
  );
  check(
    goal['description'] == 'Verify the result 中文 🙂',
    'original intent is preserved',
  );
  check(
    output['resolved']['label']['label'] == 'Update goal' &&
        output['content']['blocks'][0]['kind'] == 'notice' &&
        output['content']['blocks'][1]['path'] == '/goal/description',
    'the native message projection preserves goal status and description',
  );
  await close();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  updates = await connection.watchConversation(session: session);
  final restored = await until(updates, (_) => true);
  check(
    jsonEncode(restored['calls']) == jsonEncode(calls),
    'original goal results survive controller restart',
  );
  check(
    await connection.execute(request: request) == receipt,
    'retry preserves the original admission',
  );
  final completion = await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': original['revision'],
    'message': {
      'text': '/goal Verify the revised result 中文 🙂',
      'attachments': [],
    },
  });
  final finished = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) =>
          run['turn'] == completion['data']['id'] &&
          run['status'] == 'completed',
    ),
  );
  final replacement = (finished['calls'] as List).singleWhere(
    (call) =>
        call['name'] == env['SAILRY_UPDATE_GOAL'] &&
        call['turn'] == completion['data']['id'],
  );
  final completed = (await detail(connection, replacement))['result']['goal'];
  check(
    completed['id'] != goal['id'] &&
        completed['revision'] == '4' &&
        completed['state'] == 'completed' &&
        completed['description'] == 'Verify the revised result 中文 🙂',
    'only an explicit new request replaces the completed goal',
  );
  await close();
  RustLib.dispose();
  print('Dart FFI goal revisions and reconnect passed');
}
