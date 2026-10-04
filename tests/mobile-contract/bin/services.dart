import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/commands.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;
import 'ports.dart' show released;

Future<Map<String, dynamic>> serviceView(
  CommandUpdates updates,
  bool Function(Map<String, dynamic>) ready,
) async {
  return Future(() async {
    while (true) {
      final view = jsonDecode(await updates.next()) as Map<String, dynamic>;
      if (view['connected'] == true && ready(view)) return view;
    }
  }).timeout(const Duration(seconds: 10));
}

Future<void> main() async {
  final env = Platform.environment;
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  final controller = await Controller.open(
    path: env['SAILRY_CONTROLLER_PROFILE']!,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  final connection = await controller.connect(address: address);
  final session = env['SAILRY_SESSION']!;
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final current = (snapshot['sessions'] as List).singleWhere(
    (item) => item['id'] == session,
  );
  final config = Map<String, dynamic>.from(current['config'] as Map)
    ..['permission'] = 'full';
  final updated = (await execute(connection, 'set_session_config', {
    'session': session,
    'expected_revision': current['revision'],
    'config': config,
  }))['data'];
  final commands = await connection.watchCommands(session: session);
  final history = await connection.watchConversation(session: session);
  await execute(connection, 'submit_turn', {
    'session': session,
    'expected_revision': updated['revision'],
    'message': {'text': 'Run the test service', 'attachments': []},
  });
  final view = await serviceView(
    commands,
    (view) => (view['items'] as List).any(
      (item) => (item['services'] as List).isNotEmpty,
    ),
  );
  final command = (view['items'] as List).single;
  final service = (command['services'] as List).single;
  final source = jsonEncode({
    'session': session,
    'command': command['id'],
    'service': service,
  });
  final forwarding = await connection.forwardService(
    source: source,
    localPort: 0,
  );
  final local = await forwarding.localPort();
  check(
    jsonDecode(await forwarding.next())['kind'] == 'listening',
    'service mapping listens',
  );
  await until(
    history,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['status'] == 'completed',
    ),
  );
  final http = HttpClient();
  final request = await http.getUrl(
    Uri.parse(service['url'] as String).replace(host: '127.0.0.1', port: local),
  );
  final response = await request.close();
  check(
    await utf8.decoder.bind(response).join() == 'service:/app?q=1',
    'mapped service preserves its URL after the turn finishes',
  );
  http.close(force: true);
  await execute(connection, 'stop_command', {
    'session': session,
    'id': command['id'],
  });
  await serviceView(
    commands,
    (view) => (view['items'] as List).every(
      (item) => (item['services'] as List).isEmpty,
    ),
  );
  check(
    jsonDecode(
          await forwarding.next().timeout(const Duration(seconds: 5)),
        )['kind'] ==
        'closed',
    'shared Client closes a stopped service',
  );
  await released(local);
  var rejected = false;
  try {
    await connection.forwardService(source: source, localPort: 0);
  } catch (_) {
    rejected = true;
  }
  check(rejected, 'a stale service cannot reopen its mapping');
  await forwarding.close();
  forwarding.dispose();
  await commands.close();
  commands.dispose();
  await history.close();
  history.dispose();
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  stdout.writeln('Dart service discovery, forwarding and cleanup passed');
}
