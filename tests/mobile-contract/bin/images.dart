import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'attachments/support.dart' show downloadAttachment, revision;
import 'conversation.dart' show canonicalJson, check, execute, until;

Future<void> main() async {
  final env = Platform.environment;
  const png =
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII=';
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
  final sessions = jsonDecode(env['SAILRY_SESSIONS']!) as List;
  final session = sessions[0] as String;
  final other = sessions[1] as String;
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final worktree = (snapshot['sessions'] as List).singleWhere(
    (value) => value['id'] == session,
  )['worktree'];
  final imageFile = File('${env['SAILRY_PROJECT_PATH']}/generated.png');
  await imageFile.writeAsBytes(base64Decode(png));
  final fileDescriptor = (await execute(connection, 'download_file', {
    'worktree': worktree,
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
  final updates = await connection.watchConversation(session: session);
  final view = await until(updates, (_) => true);
  final page = view['snapshot']['page'];
  check(
    page['runs'].single['status'] == 'completed',
    'canonical image response completed: ${page['runs']}',
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
    'session': session,
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
      'data': {'session': other, 'image': image},
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
    'session': session,
    'before': null,
    'limit': 100,
  }))['data']['page'];
  check(
    canonicalJson(restored['entries']) == canonicalJson(page['entries']),
    'image history survives reconnect',
  );
  final recovered = (await execute(connection, 'download_image', {
    'session': session,
    'image': image,
  }))['data'];
  check(
    base64Encode(await downloadAttachment(connection, recovered)) == png,
    'image transfer survives reconnect',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print(
    'Dart FFI ordered text/image, authenticated download and reconnect passed',
  );
}
