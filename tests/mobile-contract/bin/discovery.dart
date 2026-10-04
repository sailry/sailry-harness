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
  final source = <String, dynamic>{
    'provider': '19089837-a2e2-424b-8dbe-4c177ecb3476',
    'api': 'anthropic',
    'endpoint': env['SAILRY_PROVIDER_ENDPOINT'],
    'credential': null,
    'secret': 'isolated-discovery-key',
  };
  final discovery = await execute(connection, 'discover_models', {
    'kind': 'draft',
    'data': source,
  });
  check(
    discovery['kind'] == 'discovered_models',
    'model discovery crosses FFI',
  );
  final catalog = discovery['data'] as Map;
  check(
    catalog['endpoint'] == source['endpoint'],
    'discovery retains the endpoint',
  );
  final models = catalog['models'] as List;
  check(
    models[0]['context'] == 4096 && models[1]['context'] == null,
    'unknown limits remain unknown',
  );
  check(
    (await execute(connection, 'list_providers', null))['data'].isEmpty,
    'draft discovery does not save',
  );
  final saved = (await execute(connection, 'save_provider', {
    'provider': {
      'id': source['provider'],
      'revision': 0,
      'api': source['api'],
      'authentication': 'api_key',
      'endpoint': source['endpoint'],
      'name': 'FFI discovery',
      'enabled': true,
      'credential': null,
      'default_model': 'known',
      'models': [
        for (final id in ['known', 'unknown', 'missing'])
          {
            'id': id,
            'context': 8192,
            'output': 64,
            'vision': false,
            'tools': false,
            'reasoning': false,
            'web_search': false,
            'generates': [],
            'efforts': [],
            'custom_efforts': false,
            'default_effort': 'default',
          },
      ],
    },
    'expected_revision': 0,
    'secret': source['secret'],
  }))['data'];
  final command = {
    'provider': saved['id'],
    'expected_revision': saved['revision'],
  };
  final report = await execute(connection, 'validate_provider', command);
  check(
    report['data']['missing'].single == 'missing',
    'missing model is explicit',
  );
  check(
    report['data']['exceeded'].single == 'known',
    'saved capacity is checked',
  );
  check(
    report['data']['unverified'].single == 'unknown',
    'unknown capacity is explicit',
  );
  check(
    !jsonEncode(report).contains(source['secret']),
    'validation does not expose credentials',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  check(
    jsonEncode(await execute(connection, 'validate_provider', command)) ==
        jsonEncode(report),
    'saved validation resumes without controller credentials',
  );
  source['secret'] = null;
  source['credential'] = saved['credential'];
  check(
    jsonEncode(
          await execute(connection, 'discover_models', {
            'kind': 'draft',
            'data': source,
          }),
        ) ==
        jsonEncode(discovery),
    'discovery resolves execution Node credentials',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI model discovery and validation passed');
}
