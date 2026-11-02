import 'dart:convert';

import 'package:sailry_bridge/api/connection.dart';

import 'conversation.dart' show check, execute;

/// Exercises the draft's shared commands before its first turn is submitted.
Future<void> selection(
  Connection connection,
  Map<String, dynamic> role,
  String id,
) async {
  final model = role['model'] as Map<String, dynamic>;
  final provider =
      ((await execute(connection, 'list_providers', null))['data'] as List)
          .firstWhere((provider) => provider['id'] == model['provider']);
  final team = (await execute(connection, 'put_team', {
    'team': {
      'id': id,
      'revision': 0,
      'key': 'reviewers',
      'name': 'Reviewers',
      'appearance': {'icon': 'code', 'color': 'blue'},
      'description': '',
      'instructions': '',
      'model': model,
      'members': [role['id']],
    },
    'expected_revision': 0,
  }))['data'];
  final created = (await execute(connection, 'create_session', {
    'project': null,
    'worktree': null,
    'config': {
      'provider': provider['id'],
      'credential': provider['credential'],
      'model': model['model'],
      'effort': 'default',
      'mode': 'code',
      'permission': 'ask',
      'options': <String, dynamic>{},
    },
  }))['data'];
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'set_session_team',
      'data': {
        'session': created['id'],
        'expected_revision': created['revision'],
        'execution': 'team',
        'team': {'id': team['id'], 'revision': team['revision']},
        'models': {role['id']: model},
      },
    }),
  );
  final receipt = await connection.execute(request: request);
  final session = jsonDecode(receipt)['Ok']['data'];
  check(session['project'] == null, 'team draft retains unassigned ownership');
  check(
    session['roles']['team']['id'] == team['id'] &&
        session['roles']['profiles'].length == 1 &&
        session['roles']['profiles'][0]['id'] == role['id'] &&
        session['roles']['models'][role['id']]['model'] == model['model'],
    'team and member models share one revision-checked command',
  );
  check(
    session['roles']['profiles'][0]['appearance']['icon'] == 'science' &&
        session['roles']['profiles'][0]['appearance']['color'] == 'green',
    'captured appearance survives the native response',
  );
  final edited = (await execute(connection, 'put_role', {
    'expected_revision': role['revision'],
    'role': {
      ...role,
      'appearance': {'icon': 'tools', 'color': 'orange'},
    },
  }))['data'];
  final catalog = (await execute(
    connection,
    'snapshot',
    null,
  ))['data']['roles'];
  check(
    catalog.single['id'] == role['id'] &&
        catalog.single['appearance']['icon'] == edited['appearance']['icon'] &&
        catalog.single['appearance']['color'] == edited['appearance']['color'],
    'current catalog appearance reaches the controller',
  );
  final restored = (await execute(connection, 'read_session', {
    'session': session['id'],
  }))['data'];
  check(
    jsonEncode(restored['roles']) == jsonEncode(session['roles']),
    'catalog edits preserve captured roles and models',
  );
  check(
    await connection.execute(request: request) == receipt,
    'team retry preserves the admitted configuration',
  );
}
