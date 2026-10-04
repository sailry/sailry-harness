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
  final output = await execute(connection, 'begin_mcp_login', {
    'package': package,
    'server': 'input',
    'redirect': 'http://127.0.0.1:43219/callback',
    'client_id': null,
  });
  final attempt = output['data'];
  var updates = await connection.watchMcpLogin(attempt: jsonEncode(attempt));
  final pending = await state(updates, (kind) => kind == 'pending');
  await updates.close();
  updates.dispose();
  updates = await connection.watchMcpLogin(attempt: jsonEncode(attempt));
  final resumed = await state(updates, (kind) => kind == 'pending');
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
      'kind': 'complete_mcp_login',
      'data': {'attempt': attempt['id'], 'callback': callback},
    }),
  );
  final receipt = await connection.execute(request: complete);
  check(jsonDecode(receipt)['Ok'] != null, 'callback admitted');
  final connected = await state(updates, (kind) => kind == 'connected');
  final current = connected['state']['data'];
  final status = await execute(connection, 'read_mcp_authorization', {
    'package': current,
    'server': 'input',
  });
  check(status['data']['configured'] == true, 'grant stored on execution Node');
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
}
