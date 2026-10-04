import 'dart:convert';
import 'package:sailry_bridge/api/connection.dart';
import 'conversation.dart' show check, execute;

Map<String, dynamic> roster(Map<String, dynamic> session) =>
    session['roles'] as Map<String, dynamic>;

Future<Map<String, dynamic>> sessionUpdate(
  Updates updates,
  String id,
  int revision,
) {
  return Future(() async {
    while (true) {
      final view = jsonDecode(await updates.next());
      check(
        view['error'] == null,
        'session configuration subscription stays healthy',
      );
      final sessions = view['snapshot']?['sessions'] as List?;
      if (sessions == null) continue;
      for (final session in sessions) {
        if (session['id'] == id && session['revision'] == revision) {
          return session as Map<String, dynamic>;
        }
      }
    }
  }).timeout(const Duration(seconds: 10));
}

void checkSourceRoles(
  Map<String, dynamic> session,
  Map<String, dynamic> state,
) {
  final roles = roster(session);
  final role = (roles['profiles'] as List).single;
  final provider = (roles['providers'] as List).single;
  check(role['key'] == 'source-review', 'import retains source role');
  check(
    role['max_turns'] == 12 && role['skills'].single == 'fixture/review',
    'role execution settings survive import',
  );
  check(
    role['instructions'] == 'Preserve the original instructions\n完整内容 🙂',
    'role instructions remain complete',
  );
  check(
    role['model']['provider'] == provider['id'] &&
        role['model']['effort'] == null,
    'fixed role resolves its private provider',
  );
  check(
    provider['default_model'] == 'ffi-source',
    'role keeps the source model',
  );
  check(
    jsonEncode(provider['credential']['node']) == jsonEncode(state['node']),
    'role credential belongs to execution Node',
  );
  check(
    !(state['providers'] as List).any((entry) => entry['id'] == provider['id']),
    'private role provider does not overwrite Node catalog',
  );
}

Future<void> verifyRoles(
  Connection connection,
  Map<String, dynamic> session,
  Map<String, dynamic> created,
) async {
  final updates = await connection.watch();
  await sessionUpdate(updates, session['id'], 1);
  final original = jsonEncode(roster(session));
  final current = roster(created);
  final selected = (current['profiles'] as List).single;
  check(
    selected['key'] == 'target-review',
    'Mobile create captures target roles',
  );
  final queued = (await execute(connection, 'queue_turn', {
    'session': session['id'],
    'expected_revision': 1,
    'message': {
      'text': 'Keep the imported role revision 中文 🙂',
      'attachments': [],
    },
  }))['data'];
  check(
    jsonEncode(queued['roles']) == original,
    'queued role revision is frozen',
  );

  final catalog = (await execute(connection, 'put_role', {
    'expected_revision': selected['revision'],
    'role': {...selected, 'instructions': 'Changed target instructions 中文 🙂'},
  }))['data'];
  final state = (await execute(connection, 'snapshot', null))['data'];
  final unchanged = (state['sessions'] as List).singleWhere(
    (s) => s['id'] == session['id'],
  );
  check(
    jsonEncode(unchanged['roles']) == original,
    'catalog edit preserves old session',
  );
  final stale = await connection.prepare(
    command: jsonEncode({
      'kind': 'set_session_roles',
      'data': {
        'session': session['id'],
        'expected_revision': 1,
        'roles': [
          {'id': selected['id'], 'revision': selected['revision']},
        ],
      },
    }),
  );
  check(
    jsonDecode(await connection.execute(request: stale))['Err']['code'] ==
        'revision_conflict',
    'stale catalog selection is rejected',
  );
  final retained = (await execute(connection, 'snapshot', null))['data'];
  check(
    jsonEncode(
          (retained['sessions'] as List).singleWhere(
            (s) => s['id'] == session['id'],
          ),
        ) ==
        jsonEncode(unchanged),
    'catalog conflict preserves the entire session revision',
  );
  final request = await connection.prepare(
    command: jsonEncode({
      'kind': 'set_session_roles',
      'data': {
        'session': session['id'],
        'expected_revision': 1,
        'roles': [
          {'id': catalog['id'], 'revision': catalog['revision']},
        ],
      },
    }),
  );
  await connection.execute(request: request);
  final result = await connection.execute(request: request);
  final revised = jsonDecode(result)['Ok']['data'];
  final observed = await sessionUpdate(updates, session['id'], 2);
  check(
    jsonEncode(observed) == jsonEncode(revised),
    'shared Client publishes frozen session roles',
  );
  check(
    revised['revision'] == 2 &&
        revised['roles']['profiles'].single['instructions'] ==
            catalog['instructions'],
    'explicit role command captures the selected revision',
  );
  check(
    jsonEncode(revised['config']) == jsonEncode(session['config']),
    'role update keeps main model configuration',
  );

  final conflict = await connection.prepare(
    command: jsonEncode({
      'kind': 'set_session_roles',
      'data': {'session': session['id'], 'expected_revision': 1, 'roles': []},
    }),
  );
  check(
    jsonDecode(await connection.execute(request: conflict))['Err']['code'] ==
        'revision_conflict',
    'stale role update is rejected',
  );
  final configured = (await execute(connection, 'set_session_config', {
    'session': session['id'],
    'expected_revision': 2,
    'config': {...revised['config'], 'permission': 'project'},
  }))['data'];
  check(
    jsonEncode(configured['roles']) == jsonEncode(revised['roles']),
    'ordinary configuration update retains role snapshot',
  );
  final next = (await execute(connection, 'queue_turn', {
    'session': session['id'],
    'expected_revision': 3,
    'message': {
      'text': 'Keep the explicitly selected role revision',
      'attachments': [],
    },
  }))['data'];
  check(
    jsonEncode(next['roles']) == jsonEncode(revised['roles']),
    'new queued turn uses the explicit revision',
  );
  final old = (await execute(connection, 'read_queued_turn', {
    'turn': queued['id'],
  }))['data'];
  check(
    jsonEncode(old['turn']['roles']) == original,
    'old queue keeps imported providers',
  );
  await execute(connection, 'remove_role', {
    'role': catalog['id'],
    'expected_revision': catalog['revision'],
  });
  check(
    await connection.execute(request: request) == result,
    'lost response recovers without reading the deleted catalog role',
  );
  await updates.close();
  updates.dispose();
}
