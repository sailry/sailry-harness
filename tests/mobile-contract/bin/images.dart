import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'attachments/support.dart' show downloadAttachment, revision;
import 'conversation.dart' show check, execute, until;

Future<void> main() async {
  final env = Platform.environment;
  const png =
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII=';
  var calls = 0;
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((request) async {
    final body = jsonDecode(await utf8.decoder.bind(request).join());
    check(body['contents'] is List, 'native Gemini request');
    check(
      request.headers.value('x-goog-api-key') == 'isolated-image-key',
      'Node resolves provider credential',
    );
    calls++;
    request.response.headers.contentType = ContentType('text', 'event-stream');
    request.response.write(
      'data: ${jsonEncode({
        'candidates': [
          {
            'content': {
              'role': 'model',
              'parts': [
                {'text': 'Before the image'},
                {
                  'inlineData': {'mimeType': 'image/png', 'data': png},
                },
                {'text': 'After the image'},
              ],
            },
            'finishReason': 'STOP',
            'index': 0,
          },
        ],
        'usageMetadata': {'promptTokenCount': 8, 'candidatesTokenCount': 4, 'totalTokenCount': 12},
        'modelVersion': 'ffi-image',
      })}\n\n',
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
  final project = (await execute(connection, 'register_project', {
    'name': 'FFI images',
    'path': env['SAILRY_PROJECT_PATH'],
  }))['data'];
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final tree = (snapshot['worktrees'] as List).firstWhere(
    (tree) => tree['project'] == project['id'],
  );
  final imageFile = File('${env['SAILRY_PROJECT_PATH']}/generated.png');
  await imageFile.writeAsBytes(base64Decode(png));
  final fileDescriptor = (await execute(connection, 'download_file', {
    'worktree': tree['id'],
    'path': 'generated.png',
  }))['data'];
  final fileDownload = await connection.downloadFile(
    download: jsonEncode(fileDescriptor),
  );
  final fileBytes = <int>[];
  try {
    while (true) {
      final chunk = await fileDownload.next();
      if (chunk == null) break;
      fileBytes.addAll(chunk);
    }
  } finally {
    await fileDownload.close();
    fileDownload.dispose();
  }
  check(base64Encode(fileBytes) == png, 'worktree image bytes cross FFI');
  final provider = (await execute(connection, 'save_provider', {
    'provider': {
      'id': '00000000-0000-4000-8000-000000000079',
      'revision': 0,
      'name': 'FFI image fixture',
      'api': 'gemini',
      'authentication': 'api_key',
      'endpoint': 'http://127.0.0.1:${server.port}/v1beta',
      'enabled': true,
      'default_model': 'ffi-image',
      'models': [
        {
          'id': 'ffi-image',
          'context': 4096,
          'output': 128,
          'vision': true,
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
    'secret': 'isolated-image-key',
  }))['data'];
  final creation = {
    'project': project['id'],
    'worktree': null,
    'config': {
      'provider': provider['id'],
      'model': 'ffi-image',
      'effort': 'default',
      'mode': 'code',
      'permission': 'ask',
      'credential': provider['credential'],
    },
  };
  final session = (await execute(
    connection,
    'create_session',
    creation,
  ))['data'];
  final other = (await execute(connection, 'create_session', creation))['data'];
  final updates = await connection.watchConversation(session: session['id']);
  await until(updates, (_) => true);
  await execute(connection, 'submit_turn', {
    'session': session['id'],
    'expected_revision': session['revision'],
    'message': {'text': 'Return text and an image', 'attachments': []},
  });
  final view = await until(
    updates,
    (snapshot) => (snapshot['page']['runs'] as List).any(
      (run) => run['status'] == 'completed' || run['status'] == 'failed',
    ),
  );
  final page = view['snapshot']['page'];
  check(
    page['runs'].single['status'] == 'completed',
    'native image response completed: ${page['runs']}',
  );
  final parts = (page['entries'] as List)
      .where((entry) => entry['author'] != 'user')
      .expand((entry) => entry['parts'] as List)
      .toList();
  check(
    parts.map((part) => part['kind']).join(',') == 'text,image,text',
    'ordered text and image parts cross FFI',
  );
  check(
    parts.first['data'] == 'Before the image' &&
        parts.last['data'] == 'After the image',
    'surrounding text retained',
  );
  final image = Map<String, dynamic>.from(parts[1]['data']);
  check(
    !jsonEncode(page).contains(png) && !image.containsKey('bytes'),
    'history contains image metadata only',
  );
  final descriptor = (await execute(connection, 'download_image', {
    'session': session['id'],
    'image': image,
  }))['data'];
  final bytes = await downloadAttachment(connection, descriptor);
  check(base64Encode(bytes) == png, 'authenticated image bytes cross FFI');
  check(
    await revision(bytes) == image['attachment']['spec']['revision'],
    'image digest verified',
  );
  final forged = await connection.prepare(
    command: jsonEncode({
      'kind': 'download_image',
      'data': {'session': other['id'], 'image': image},
    }),
  );
  check(
    jsonDecode(await connection.execute(request: forged))['Err']['code'] ==
        'not_found',
    'another session cannot read the image',
  );
  await updates.close();
  updates.dispose();
  await controller.close();
  connection.dispose();
  controller.dispose();
  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  final restored = (await execute(connection, 'read_conversation', {
    'session': session['id'],
    'before': null,
    'limit': 100,
  }))['data']['page'];
  check(
    jsonEncode(restored['entries']) == jsonEncode(page['entries']),
    'image history survives reconnect',
  );
  final recovered = (await execute(connection, 'download_image', {
    'session': session['id'],
    'image': image,
  }))['data'];
  check(
    base64Encode(await downloadAttachment(connection, recovered)) == png,
    'image transfer survives reconnect',
  );
  check(calls == 1, 'history and download do not replay the provider');
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  await server.close(force: true);
  RustLib.dispose();
  print(
    'Dart FFI ordered text/image, authenticated download and reconnect passed',
  );
}
