import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/login.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;

void publicOnly(Object? value) {
  final text = jsonEncode(value);
  for (final secret in [
    'ffi-device-secret',
    'ffi-code-secret',
    'ffi-verifier-secret',
    'ffi-refresh-secret',
    'ffi-github-secret',
    'ffi-copilot-access',
    'ffi-account',
  ]) {
    check(!text.contains(secret), 'authorization secrets stay on Node');
  }
}

Future<Map<String, dynamic>> state(
  LoginUpdates updates,
  bool Function(String) ready,
) async {
  return Future(() async {
    while (true) {
      final view = jsonDecode(await updates.next()) as Map<String, dynamic>;
      publicOnly(view);
      check(view['error'] == null, 'login observation succeeds');
      final update = view['update'] as Map<String, dynamic>?;
      if (update == null) continue;
      final status = update['state'] as Map<String, dynamic>;
      if (ready(status['kind'] as String)) return update;
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
  final authentication = env['SAILRY_AUTHENTICATION']!;
  var provider = (await execute(connection, 'put_provider', {
    'expected_revision': 0,
    'provider': {
      'id': '4f71b629-72f0-4154-a741-3456b7bf6c42',
      'revision': 0,
      'name': 'FFI sign-in',
      'api': 'responses',
      'authentication': authentication,
      'endpoint': authentication == 'chat_gpt'
          ? 'https://chatgpt.com/backend-api/codex'
          : 'https://api.githubcopilot.com',
      'enabled': true,
      'credential': null,
      'default_model': 'ffi-login',
      'models': [
        {
          'id': 'ffi-login',
          'context': 4096,
          'output': 128,
          'tools': false,
          'vision': false,
          'reasoning': false,
          'web_search': false,
          'generates': [],
          'efforts': [],
          'custom_efforts': false,
          'default_effort': 'default',
        },
      ],
    },
  }))['data'];
  for (var index = 0; index < 3; index++) {
    final request = await connection.prepare(
      command: jsonEncode({
        'kind': 'begin_provider_login',
        'data': {
          'provider': provider['id'],
          'expected_revision': provider['revision'],
        },
      }),
    );
    final result = await connection.execute(request: request);
    final output = jsonDecode(result)['Ok'];
    check(
      output?['kind'] == 'provider_login',
      'device login reaches durable Node admission',
    );
    publicOnly(output);
    final attempt = output['data'];
    var updates = await connection.watchLogin(attempt: jsonEncode(attempt));
    final pending = await state(updates, (kind) => kind == 'pending');
    check(
      pending['state']['data']['user_code'] == 'FFI-1234',
      'device code crosses FFI',
    );
    if (index == 0) {
      await execute(connection, 'cancel_provider_login', {
        'attempt': attempt['id'],
      });
      await state(updates, (kind) => kind == 'cancelled');
    } else {
      await updates.close();
      updates.dispose();
      if (index == 2) {
        await connection.close();
        connection.dispose();
        await controller.close();
        controller.dispose();
        controller = await Controller.open(
          path: path,
          internet: false,
          relays: [],
        );
        connection = await controller.connect(address: address);
      }
      updates = await connection.watchLogin(attempt: jsonEncode(attempt));
      await state(updates, (kind) => kind == 'connected');
    }
    check(
      await connection.execute(request: request) == result,
      'receipt recovery never begins another login',
    );
    await updates.close();
    var closed = false;
    try {
      await updates.next();
    } catch (_) {
      closed = true;
    }
    check(closed, 'closed login handles reject reads');
    updates.dispose();
    provider = (await execute(
      connection,
      'list_providers',
      null,
    ))['data'].single;
  }
  var invalid = false;
  final discovered = await execute(connection, 'discover_models', {
    'kind': 'saved',
    'data': {
      'provider': provider['id'],
      'expected_revision': provider['revision'],
    },
  });
  publicOnly(discovered);
  check(
    discovered['kind'] == 'discovered_models',
    'model discovery crosses the shared contract',
  );
  final model = (discovered['data']['models'] as List).single;
  check(model['id'] == 'ffi-login', 'discovery uses execution account');
  check(
    model['context'] == 8192 && model['output'] == 512,
    'native limits cross the shared contract',
  );
  final capabilities = model['capabilities'];
  check(
    capabilities['vision'] == true && capabilities['reasoning'] == true,
    'native capabilities cross FFI',
  );
  check(
    jsonEncode(capabilities['efforts']) == '["low","high"]',
    'native reasoning choices remain explicit',
  );
  check(
    capabilities['default_effort'] ==
        (authentication == 'chat_gpt' ? 'high' : null),
    'unknown defaults remain unknown',
  );
  final validation = (await execute(connection, 'validate_provider', {
    'provider': provider['id'],
    'expected_revision': provider['revision'],
  }))['data'];
  publicOnly(validation);
  check(
    validation['missing'].isEmpty &&
        validation['exceeded'].isEmpty &&
        validation['unverified'].isEmpty,
    'saved account configuration validates',
  );
  check(
    jsonEncode(
          (await execute(connection, 'list_providers', null))['data'].single,
        ) ==
        jsonEncode(provider),
    'catalog reads preserve saved configuration',
  );
  try {
    await connection.watchLogin(attempt: '{}');
  } catch (_) {
    invalid = true;
  }
  check(invalid, 'malformed attempts are rejected');
  final project = (await execute(connection, 'register_project', {
    'name': 'FFI authorization',
    'path': env['SAILRY_PROJECT_PATH'],
  }))['data'];
  final session = (await execute(connection, 'create_session', {
    'project': project['id'],
    'worktree': null,
    'config': {
      'provider': provider['id'],
      'model': 'ffi-login',
      'effort': 'default',
      'mode': 'code',
      'permission': 'ask',
      'credential': provider['credential'],
    },
  }))['data'];
  final history = await connection.watchConversation(session: session['id']);
  await until(history, (_) => true);
  final turn = (await execute(connection, 'submit_turn', {
    'session': session['id'],
    'expected_revision': session['revision'],
    'message': {'text': 'Run the authorized fixture', 'attachments': []},
  }))['data'];
  final completed = await until(
    history,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['turn'] == turn['id'] && run['status'] == 'completed',
    ),
  );
  publicOnly(completed);
  check(
    jsonEncode(completed).contains('FFI authorized response'),
    'ADK uses the Node login after controller restart',
  );
  await history.close();
  history.dispose();
  publicOnly(await execute(connection, 'snapshot', null));
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print('Dart FFI device login lifecycle passed');
}
