import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute;

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
  check(
    (await execute(
          connection,
          'read_catalog_status',
          null,
        ))['data']['revision'] ==
        0,
    'fresh catalog is explicit',
  );
  final updates = await connection.watch();
  await updates.next();
  final first = (await execute(
    connection,
    'refresh_model_catalog',
    null,
  ))['data'];
  check(
    first['revision'] == 1 && first['models'] == 2,
    'catalog refresh crosses FFI',
  );
  while (true) {
    final view = jsonDecode(
      await updates.next().timeout(const Duration(seconds: 5)),
    );
    if (view['snapshot']?['model_catalog']?['revision'] == 1) break;
  }
  await updates.close();
  updates.dispose();
  final query = <String, dynamic>{
    'ids': [],
    'provider': 'anthropic',
    'revision': null,
    'after': null,
    'limit': 1,
  };
  final page = (await execute(connection, 'read_model_catalog', query))['data'];
  final model = page['models'].single;
  check(
    model['id'] == 'known' &&
        model['context'] == 8192 &&
        model['tools'] == true,
    'reference capabilities cross FFI',
  );
  check(
    model['options'].single['type'] == 'budget_tokens' &&
        model['options'].single['min'] == 1024,
    'native reference options remain explicit',
  );
  query['revision'] = page['revision'];
  query['after'] = page['next'];
  final next = (await execute(connection, 'read_model_catalog', query))['data'];
  check(
    next['models'].single['id'] == 'unknown' &&
        next['models'].single['context'] == null &&
        next['next'] == null,
    'unknown limits remain unknown across pages',
  );
  final current = (await execute(
    connection,
    'refresh_model_catalog',
    null,
  ))['data'];
  final stale = await connection.prepare(
    command: jsonEncode({'kind': 'read_model_catalog', 'data': query}),
  );
  check(
    jsonDecode(await connection.execute(request: stale))['Err']['code'] ==
        'revision_conflict',
    'stale pagination is rejected',
  );
  final refresh = await connection.prepare(
    command: jsonEncode({'kind': 'refresh_model_catalog'}),
  );
  check(
    jsonDecode(await connection.execute(request: refresh))['Err'] != null,
    'failed refresh is explicit',
  );
  check(
    jsonEncode(
          (await execute(connection, 'read_catalog_status', null))['data'],
        ) ==
        jsonEncode(current),
    'failure preserves old cache',
  );
  check(
    (await execute(connection, 'list_providers', null))['data'].isEmpty,
    'reference refresh does not save provider configuration',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  check(
    jsonEncode(
          (await execute(connection, 'read_catalog_status', null))['data'],
        ) ==
        jsonEncode(current),
    'new controller resumes execution catalog',
  );
  check(
    (await execute(
          connection,
          'snapshot',
          null,
        ))['data']['model_catalog']['revision'] ==
        2,
    'shared snapshot recovers catalog',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI reference catalog passed');
}
