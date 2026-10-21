import 'dart:convert';
import 'dart:io';
import 'package:sailry_bridge/api/connection.dart';
import 'conversation.dart' show check, execute;
import 'login.dart' show state;

Future<void> authorizeMcp(Connection connection) async {
  final info = (await execute(connection, 'read_plugin', {
    'name': 'example',
  }))['data'];
  final summary = info['summary'];
  final package = {
    'name': summary['name'],
    'digest': summary['digest'],
    'settings_revision': summary['settings_revision'],
  };
  final connections = await connection.watchConnections();
  final initial = jsonDecode(await connections.next()) as Map<String, dynamic>;
  check(
    (initial['types'] as List).any(
      (type) =>
          type['package']['name'] == 'example' &&
          type['declaration']['id'] == 'service',
    ),
    'connection snapshot includes declared types',
  );
  final output = await execute(connection, 'start_connector_login', {
    'package': package,
    'connector': 'service',
    'connection': null,
    'expected_revision': null,
    'method': 'mcp_oauth',
    'input': {'redirect': 'http://127.0.0.1:43219/auth/callback'},
  });
  final attempt = output['data'];
  var updates = await connection.watchConnectorLogin(
    attempt: jsonEncode(attempt),
  );
  final pending = await state(updates, (kind) => kind == 'browser');
  await updates.close();
  updates.dispose();
  updates = await connection.watchConnectorLogin(attempt: jsonEncode(attempt));
  final resumed = await state(updates, (kind) => kind == 'browser');
  check(
    resumed['revision'] == pending['revision'],
    'observation does not restart OAuth',
  );
  final http = HttpClient();
  final request = await http.getUrl(
    Uri.parse(pending['state']['data']['url'] as String),
  );
  request.followRedirects = false;
  final response = await request.close();
  final callback = response.headers.value('location')!;
  await response.drain<void>();
  http.close();
  final complete = await connection.prepare(
    command: jsonEncode({
      'kind': 'submit_connector_input',
      'data': {'attempt': attempt['id'], 'input': callback},
    }),
  );
  final receipt = await connection.execute(request: complete);
  check(jsonDecode(receipt)['Ok'] != null, 'callback admitted');
  final connected = await state(updates, (kind) => kind == 'connected');
  final current = connected['state']['data'];
  final status = await execute(connection, 'list_connections', null);
  check(
    (status['data'] as List).any(
      (row) => row['id'] == current['id'] && row['state'] == 'connected',
    ),
    'grant stored on execution Node',
  );
  await Future(() async {
    while (true) {
      final snapshot =
          jsonDecode(await connections.next()) as Map<String, dynamic>;
      if ((snapshot['connections'] as List).any(
        (row) => row['id'] == current['id'] && row['state'] == 'connected',
      )) {
        check(
          snapshot['cursor'] > initial['cursor'],
          'connection cursor advances',
        );
        break;
      }
    }
  }).timeout(const Duration(seconds: 10));
  for (final secret in ['access-initial', 'refresh-initial', 'fixture-code']) {
    check(
      !jsonEncode([pending, connected, status]).contains(secret),
      'grant is absent from controller projection',
    );
  }
  check(
    await connection.execute(request: complete) == receipt,
    'callback exchange is not repeated',
  );
  await updates.close();
  updates.dispose();
  await connections.close();
  connections.dispose();
}
