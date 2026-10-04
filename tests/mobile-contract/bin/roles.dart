import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute;

Future<List<dynamic>> catalog(
  Updates updates,
  bool Function(List<dynamic>) ready,
) {
  return Future(() async {
    while (true) {
      final view = jsonDecode(await updates.next());
      check(view['error'] == null, 'catalog subscription remains healthy');
      final roles = view['snapshot']?['roles'] as List<dynamic>?;
      if (roles != null && ready(roles)) return roles;
    }
  }).timeout(const Duration(seconds: 10));
}

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
  var updates = await connection.watch();
  await catalog(updates, (roles) => roles.isEmpty);

  Future<String> prepare(String kind, Map<String, dynamic> data) =>
      connection.prepare(command: jsonEncode({'kind': kind, 'data': data}));
  Future<void> rejects(
    String kind,
    Map<String, dynamic> data,
    String code,
  ) async {
    final request = await prepare(kind, data);
    final result = jsonDecode(await connection.execute(request: request));
    check(result['Err']?['code'] == code, 'role command reports $code');
  }

  final role = <String, dynamic>{
    'id': env['SAILRY_ROLE'],
    'revision': 0,
    'key': 'review',
    'name': 'Review 中文',
    'description': 'Inspect changes',
    'model': null,
    'max_turns': 12,
    'skills': ['local-review'],
    'instructions': 'Review the supplied changes\n保留完整内容',
  };
  final create = await prepare('put_role', {
    'role': role,
    'expected_revision': 0,
  });
  // Discard the application response, then recover using the unchanged request.
  await connection.execute(request: create);
  final created = await connection.execute(request: create);
  final saved = jsonDecode(created)['Ok']['data'];
  check(
    saved['revision'] == 1 && saved['model'] == null,
    'inheritance persists',
  );
  var roles = await catalog(updates, (roles) => roles.length == 1);
  check(
    jsonEncode(roles.single) == jsonEncode(saved),
    'Client projection matches receipt',
  );
  await rejects('put_role', {
    'role': {...role, 'id': env['SAILRY_REPLACEMENT_ROLE']},
    'expected_revision': 0,
  }, 'conflict');
  await rejects('put_role', {
    'role': role,
    'expected_revision': 0,
  }, 'revision_conflict');
  await rejects('put_role', {
    'role': {
      ...role,
      'skills': ['one', 'two', 'three', 'four'],
    },
    'expected_revision': 1,
  }, 'invalid_request');

  final fixed = <String, dynamic>{
    ...role,
    'key': 'review-renamed',
    'model': {
      'provider': env['SAILRY_ROLE_PROVIDER'],
      'model': 'ffi-role',
      'effort': null,
    },
  };
  final edited = (await execute(connection, 'put_role', {
    'role': fixed,
    'expected_revision': 1,
  }))['data'];
  check(
    edited['id'] == saved['id'] && edited['revision'] == 2,
    'rename preserves identity',
  );
  roles = await catalog(
    updates,
    (roles) => roles.length == 1 && roles.single['revision'] == 2,
  );
  check(
    jsonEncode(roles.single) == jsonEncode(edited),
    'fixed model and Unicode survive events',
  );
  check(
    await connection.execute(request: create) == created,
    'old receipt stays immutable',
  );
  check(
    (await execute(
          connection,
          'list_roles',
          null,
        ))['data'].single['revision'] ==
        2,
    'retry does not revert configuration',
  );
  await rejects('put_role', {
    'role': {
      ...fixed,
      'model': {...fixed['model'], 'model': 'missing'},
    },
    'expected_revision': 2,
  }, 'not_configured');
  await rejects('put_role', {
    'role': {
      ...fixed,
      'model': {...fixed['model'], 'effort': 'high'},
    },
    'expected_revision': 2,
  }, 'invalid_request');
  await rejects('remove_role', {
    'role': role['id'],
    'expected_revision': 1,
  }, 'revision_conflict');
  final remove = await prepare('remove_role', {
    'role': role['id'],
    'expected_revision': 2,
  });
  await connection.execute(request: remove);
  await catalog(updates, (roles) => roles.isEmpty);
  final removed = await connection.execute(request: remove);
  final replacement = (await execute(connection, 'put_role', {
    'role': {...fixed, 'id': env['SAILRY_REPLACEMENT_ROLE'], 'key': 'review'},
    'expected_revision': 0,
  }))['data'];
  roles = await catalog(updates, (roles) => roles.length == 1);
  check(
    jsonEncode(roles.single) == jsonEncode(replacement),
    'replacement has independent identity',
  );
  check(
    await connection.execute(request: remove) == removed,
    'remove retry cannot delete replacement',
  );

  final pending = updates.next().then((_) => false, onError: (_) => true);
  await updates.close();
  check(
    await pending.timeout(const Duration(seconds: 2)),
    'catalog subscription releases',
  );
  updates.dispose();
  await controller.close();
  connection.dispose();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  updates = await connection.watch();
  roles = await catalog(updates, (roles) => roles.length == 1);
  check(
    jsonEncode(roles.single) == jsonEncode(replacement),
    'reopen restores Node catalog',
  );
  check(
    await connection.execute(request: create) == created,
    'create receipt survives reopen',
  );
  check(
    await connection.execute(request: remove) == removed,
    'remove receipt survives reopen',
  );
  final state = (await execute(connection, 'snapshot', null))['data'];
  check(
    jsonEncode(state['roles'].single) == jsonEncode(replacement),
    'snapshot remains current',
  );
  check(
    !jsonEncode(state).contains('isolated-ffi-configuration-credential'),
    'no credentials in snapshot',
  );
  await updates.close();
  updates.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print(
    'Dart FFI role configuration, recovery and catalog subscription passed',
  );
}
