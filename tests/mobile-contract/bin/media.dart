import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check, execute, until;

Future<void> main() async {
  final env = Platform.environment;
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  final accepted = Completer<void>();
  var submissions = 0;
  server.listen((request) async {
    if (request.method == 'POST') {
      final body = jsonDecode(await utf8.decoder.bind(request).join());
      check(
        body['model'] == 'video-fixture',
        'execution Node uses selected media model',
      );
      check(
        request.headers.value('authorization') == 'Bearer isolated-media-key',
        'execution Node resolves media credentials',
      );
      submissions++;
      if (!accepted.isCompleted) accepted.complete();
    }
    request.response.headers.contentType = ContentType.json;
    request.response.write(
      jsonEncode({'id': 'pending', 'status': 'in_progress'}),
    );
    await request.response.close();
  });
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
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final main = snapshot['providers'].single;
  final provider = Map<String, dynamic>.from(main);
  provider.addAll({
    'id': '00000000-0000-4000-8000-000000000078',
    'revision': 0,
    'name': 'FFI media',
    'endpoint': 'http://127.0.0.1:${server.port}/v1',
    'credential': null,
    'default_model': 'video-fixture',
  });
  provider['models'] = [
    {
      ...Map<String, dynamic>.from(main['models'].single),
      'id': 'video-fixture',
      'tools': false,
      'generates': ['video'],
    },
  ];
  await execute(connection, 'save_provider', {
    'provider': provider,
    'expected_revision': 0,
    'secret': 'isolated-media-key',
  });
  final settings = (await execute(connection, 'save_media_settings', {
    'revision': 0,
    'bindings': {
      'video': {'provider': provider['id'], 'model': 'video-fixture'},
    },
  }))['data'];
  final updates = await connection.watch();
  final projected = jsonDecode(
    await updates.next().timeout(const Duration(seconds: 10)),
  );
  check(
    projected['snapshot']['media_settings']['revision'] == 1,
    'shared Client projects media settings',
  );
  check(
    !jsonEncode(projected).contains('isolated-media-key'),
    'media snapshots exclude credentials',
  );
  await updates.close();
  updates.dispose();
  final invalid = await connection.prepare(
    command: jsonEncode({
      'kind': 'save_media_settings',
      'data': {
        'revision': 1,
        'bindings': {
          'vision': {'provider': provider['id'], 'model': 'video-fixture'},
        },
      },
    }),
  );
  check(
    jsonDecode(await connection.execute(request: invalid))['Err']['code'] ==
        'invalid_request',
    'incompatible media capability is rejected',
  );
  for (final permission in ['ask', 'full']) {
    final session = (await execute(connection, 'create_session', {
      'project': snapshot['projects'].single['id'],
      'worktree': snapshot['worktrees'].single['id'],
      'config': {
        'provider': main['id'],
        'model': main['default_model'],
        'effort': 'default',
        'mode': 'code',
        'permission': permission,
        'credential': main['credential'],
      },
    }))['data'];
    var observer = await connection.watchConversation(session: session['id']);
    final request = await connection.prepare(
      command: jsonEncode({
        'kind': 'submit_turn',
        'data': {
          'session': session['id'],
          'expected_revision': 1,
          'message': {'text': 'Generate a video', 'attachments': []},
        },
      }),
    );
    final result = await connection.execute(request: request);
    final turn = jsonDecode(result)['Ok']['data']['id'];
    if (permission == 'ask') {
      final view = await until(
        observer,
        (snapshot) => (snapshot['page']['approvals'] as List).any(
          (approval) => approval['state'] == 'pending',
        ),
      );
      final approval = (view['snapshot']['page']['approvals'] as List)
          .singleWhere((approval) => approval['state'] == 'pending');
      await execute(connection, 'resolve_approval', {
        'session': session['id'],
        'approval': approval['id'],
        'decision': 'deny',
      });
      await until(
        observer,
        (snapshot) => (snapshot['page']['runs'] as List).any(
          (run) => run['turn'] == turn && run['status'] == 'completed',
        ),
      );
      check(submissions == 0, 'denied generation never reaches provider');
    } else {
      await accepted.future.timeout(const Duration(seconds: 10));
      await observer.close();
      observer.dispose();
      await controller.close();
      connection.dispose();
      controller.dispose();
      controller = await Controller.open(
        path: path,
        internet: false,
        relays: [],
      );
      connection = await controller.connect(address: address);
      observer = await connection.watchConversation(session: session['id']);
      await until(
        observer,
        (snapshot) => (snapshot['page']['runs'] as List).any(
          (run) => run['turn'] == turn && run['status'] == 'running',
        ),
      );
      await execute(connection, 'save_media_settings', {
        'revision': settings['revision'],
        'bindings': {},
      });
      await execute(connection, 'stop_turn', {'turn': turn});
      await until(
        observer,
        (snapshot) => (snapshot['page']['runs'] as List).any(
          (run) => run['turn'] == turn && run['status'] == 'cancelled',
        ),
      );
      check(
        await connection.execute(request: request) == result,
        'reconnected request does not replay generation',
      );
      check(submissions == 1, 'provider job is submitted once');
    }
    await observer.close();
    observer.dispose();
  }
  await controller.close();
  connection.dispose();
  controller.dispose();
  await server.close(force: true);
  RustLib.dispose();
  stdout.writeln(
    'Dart media configuration, approval, reconnect and stop passed',
  );
}
