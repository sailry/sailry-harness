import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'approvals.dart' show hasPending, pending, part;

Future<List<dynamic>> children(Updates updates, String parent) async {
  return Future(() async {
    while (true) {
      final view = jsonDecode(await updates.next());
      check(view['error'] == null, 'Node subscription remains healthy');
      final snapshot = view['snapshot'];
      if (snapshot == null) continue;
      final children = (snapshot['sessions'] as List)
          .where((session) => session['delegation']?['session'] == parent)
          .toList();
      final ids = children.map((session) => session['id']).toSet();
      final turns = (snapshot['turns'] as List).where(
        (turn) => ids.contains(turn['session']),
      );
      if (children.length == 2 && turns.length == 2) return children;
    }
  }).timeout(const Duration(seconds: 10));
}

bool ended(Map<String, dynamic> snapshot, String status) =>
    (snapshot['page']['runs'] as List).single['status'] == status;

Future<void> rejects(
  Connection connection,
  String kind,
  Map<String, dynamic> data,
  String code,
) async {
  final request = await connection.prepare(
    command: jsonEncode({'kind': kind, 'data': data}),
  );
  check(
    jsonDecode(await connection.execute(request: request))['Err']?['code'] ==
        code,
    'child command reports $code',
  );
}

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final path = env['SAILRY_CONTROLLER_PROFILE']!;
  final sessions = jsonDecode(env['SAILRY_SESSIONS']!) as List;
  final session = sessions[0] as String;
  var controller = await Controller.open(
    path: path,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  var workspace = await connection.watch();
  var parentUpdates = await connection.watchConversation(session: session);
  await until(parentUpdates, (_) => true);
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'submit_turn',
      'data': {
        'session': session,
        'expected_revision': 2,
        'message': {
          'text': 'Parent-only context: delegate both tasks',
          'attachments': [],
        },
      },
    }),
  );
  final receipt = await connection.execute(request: request);
  final parentTurn = jsonDecode(receipt)['Ok']['data']['id'];
  final descendants = await children(workspace, session);
  var firstUpdates = await connection.watchConversation(
    session: descendants[0]['id'],
  );
  var secondUpdates = await connection.watchConversation(
    session: descendants[1]['id'],
  );
  final firstView = await until(firstUpdates, hasPending);
  final secondView = await until(secondUpdates, hasPending);
  final first = pending(firstView);
  final second = pending(secondView);
  check(
    first['id'] != second['id'] && first['turn'] != second['turn'],
    'child approvals have independent identities',
  );
  final parentView = await until(
    parentUpdates,
    (snapshot) =>
        (snapshot['page']['children'] as List).length == 2 &&
        (snapshot['page']['entries'] as List).any(
          (entry) =>
              (entry['parts'] as List)
                  .where((part) => part['kind'] == 'tool_call')
                  .length ==
              2,
        ),
  );
  for (var index = 0; index < descendants.length; index++) {
    final child = descendants[index];
    final origin = child['delegation'];
    final summary = (parentView['snapshot']['page']['children'] as List)
        .singleWhere((summary) => summary['run']['session'] == child['id']);
    check(
      jsonEncode(summary['origin']) == jsonEncode(origin) &&
          summary['name'] == 'Review 中文' &&
          summary['run']['status'] == 'running',
      'parent projection includes the frozen role and running child',
    );
    check(
      origin['turn'] == parentTurn && origin['role'] != null,
      'child belongs to the admitted parent turn and role',
    );
    final call = part(parentView, origin);
    check(
      call['name'] == env['SAILRY_SPAWN_TOOL'] &&
          call['arguments']['role'] == 'review',
      'ancestry references a canonical parent call',
    );
    final view = index == 0 ? firstView : secondView;
    final approval = pending(view);
    final write = part(view, approval);
    check(
      write['id'] == 'call-fixture-1',
      'reused model call IDs remain isolated by child turn',
    );
    check(
      write['arguments']['text'] == List.filled(200, '完整子任务结果 🙂\n').join(),
      'full child tool arguments survive FFI',
    );
    final read = (view['calls'] as List).first;
    check(
      part(view, read['response'])['result']['data']['text'] == 'Read 子任务 🙂',
      'child uses its bound worktree',
    );
  }
  await rejects(connection, 'resolve_approval', {
    'session': descendants[0]['id'],
    'approval': second['id'],
    'decision': 'approve',
  }, 'wrong_target');
  await rejects(connection, 'submit_turn', {
    'session': descendants[0]['id'],
    'expected_revision': 1,
    'message': {'text': 'Cannot resume a child', 'attachments': []},
  }, 'permission_denied');
  await firstUpdates.close();
  firstUpdates.dispose();
  await secondUpdates.close();
  secondUpdates.dispose();
  await parentUpdates.close();
  parentUpdates.dispose();
  await workspace.close();
  workspace.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();

  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  workspace = await connection.watch();
  final restoredChildren = await children(workspace, session);
  for (final original in descendants) {
    final restored = restoredChildren.singleWhere(
      (child) => child['id'] == original['id'],
    );
    // Activity can advance from admission to an approval while disconnected.
    // Frozen child identity/configuration must remain exactly the same.
    Map<String, dynamic> frozen(Map<String, dynamic> child) =>
        {...child}..remove('activity');
    check(
      jsonEncode(frozen(restored)) == jsonEncode(frozen(original)),
      'frozen child metadata survives controller reopen',
    );
    check(
      restored['activity']['run']['status'] == 'running' &&
          restored['activity']['waiting'] == 'approval',
      'reopened child activity reflects its pending approval',
    );
  }
  parentUpdates = await connection.watchConversation(session: session);
  firstUpdates = await connection.watchConversation(
    session: descendants[0]['id'],
  );
  secondUpdates = await connection.watchConversation(
    session: descendants[1]['id'],
  );
  check(
    pending(await until(firstUpdates, hasPending))['id'] == first['id'],
    'first pending child survives reopen',
  );
  check(
    pending(await until(secondUpdates, hasPending))['id'] == second['id'],
    'second pending child survives reopen',
  );
  final allow = await connection.prepare(
    command: jsonEncode({
      'kind': 'resolve_approval',
      'data': {
        'session': descendants[0]['id'],
        'approval': first['id'],
        'decision': 'approve',
      },
    }),
  );
  await connection.execute(request: allow);
  final allowed = await connection.execute(request: allow);
  await execute(connection, 'resolve_approval', {
    'session': descendants[1]['id'],
    'approval': second['id'],
    'decision': 'deny',
  });
  final firstDone = await until(
    firstUpdates,
    (snapshot) => ended(snapshot, 'completed'),
  );
  final secondDone = await until(
    secondUpdates,
    (snapshot) => ended(snapshot, 'completed'),
  );
  final parentDone = await until(
    parentUpdates,
    (snapshot) => ended(snapshot, 'completed'),
  );
  check(
    firstDone['calls'][1]['approval']['state'] == 'approved' &&
        secondDone['calls'][1]['approval']['state'] == 'denied',
    'child decisions remain separate',
  );
  final childIds = descendants.map((session) => session['id']).toSet();
  check(
    (parentDone['snapshot']['page']['children'] as List).every(
      (summary) =>
          childIds.contains(summary['run']['session']) &&
          summary['run']['status'] == 'completed',
    ),
    'parent subscription receives completed child summaries',
  );
  final returned = (parentDone['calls'] as List)
      .map((call) => part(parentDone, call['response'])['result'])
      .toList();
  check(
    returned.every(
      (result) =>
          childIds.contains(result['session']) &&
          result['status'] == 'completed' &&
          result['response'] == 'answer-ffi-child',
    ),
    'parent receives actual child completions',
  );
  final written = await execute(connection, 'read_file', {
    'worktree': descendants[0]['worktree'],
    'path': 'child.txt',
  });
  check(
    written['data']['text'] == List.filled(200, '完整子任务结果 🙂\n').join(),
    'only approved child writes the complete file',
  );
  check(
    await connection.execute(request: request) == receipt,
    'lost submit receipt recovers without new children',
  );
  await firstUpdates.close();
  firstUpdates.dispose();
  await secondUpdates.close();
  secondUpdates.dispose();
  await parentUpdates.close();
  parentUpdates.dispose();

  final cancelledSession = sessions[1] as String;
  parentUpdates = await connection.watchConversation(session: cancelledSession);
  final turn = (await execute(connection, 'submit_turn', {
    'session': cancelledSession,
    'expected_revision': 2,
    'message': {
      'text': 'Parent-only context: wait for approval',
      'attachments': [],
    },
  }))['data']['id'];
  final cancelledChildren = await children(workspace, cancelledSession);
  firstUpdates = await connection.watchConversation(
    session: cancelledChildren[0]['id'],
  );
  secondUpdates = await connection.watchConversation(
    session: cancelledChildren[1]['id'],
  );
  final firstPending = pending(await until(firstUpdates, hasPending));
  final secondPending = pending(await until(secondUpdates, hasPending));
  await execute(connection, 'stop_turn', {'turn': firstPending['turn']});
  final stoppedChild = await until(
    firstUpdates,
    (snapshot) => ended(snapshot, 'cancelled'),
  );
  check(
    stoppedChild['calls'][1]['approval']['state'] == 'cancelled',
    'stopping one child closes only its pending call',
  );
  final other = await execute(connection, 'read_conversation', {
    'session': cancelledChildren[1]['id'],
    'before': null,
    'limit': 100,
  });
  check(
    other['data']['page']['runs'][0]['status'] == 'running',
    'sibling remains running after one child stops',
  );
  await execute(connection, 'stop_turn', {'turn': turn});
  final stoppedParent = await until(
    parentUpdates,
    (snapshot) => ended(snapshot, 'cancelled'),
  );
  check(
    (stoppedParent['snapshot']['page']['children'] as List).length == 2 &&
        (stoppedParent['snapshot']['page']['children'] as List).every(
          (summary) => summary['run']['status'] == 'cancelled',
        ),
    'parent subscription receives both independent and propagated stops',
  );
  final stoppedSibling = await until(
    secondUpdates,
    (snapshot) => ended(snapshot, 'cancelled'),
  );
  check(
    stoppedSibling['calls'][1]['approval']['state'] == 'cancelled',
    'parent stop drains the remaining child',
  );
  await rejects(connection, 'resolve_approval', {
    'session': cancelledChildren[1]['id'],
    'approval': secondPending['id'],
    'decision': 'approve',
  }, 'conflict');
  await firstUpdates.close();
  firstUpdates.dispose();
  await secondUpdates.close();
  secondUpdates.dispose();
  await parentUpdates.close();
  parentUpdates.dispose();
  await workspace.close();
  workspace.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();

  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  for (final saved in [parentDone, firstDone, secondDone]) {
    final id = saved['snapshot']['page']['session'] as String;
    final updates = await connection.watchConversation(session: id);
    final restored = await until(updates, (_) => true);
    check(
      jsonEncode(restored['snapshot']['page']) ==
              jsonEncode(saved['snapshot']['page']) &&
          jsonEncode(restored['calls']) == jsonEncode(saved['calls']),
      'reopened child history and call projection remain unchanged',
    );
    await updates.close();
    updates.dispose();
  }
  check(
    await connection.execute(request: allow) == allowed,
    'original child approval receipt remains recoverable',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print(
    'Dart FFI child ancestry, approvals, independent stop and recovery passed',
  );
}
