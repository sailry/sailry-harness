import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/transfers.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'attachments/support.dart';
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
  final project = (await execute(connection, 'register_project', {
    'name': 'FFI attachments',
    'path': env['SAILRY_PROJECT_PATH'],
  }))['data'];
  final snapshot = (await execute(connection, 'snapshot', null))['data'];
  final worktree =
      (snapshot['worktrees'] as List).singleWhere(
            (tree) => tree['project'] == project['id'],
          )['id']
          as String;
  final binary = Uint8List.fromList(
    List.generate(10 * 1024 * 1024 + 17, (index) => index % 256),
  );
  late Map<String, dynamic> saved;
  late String publish;
  late String published;
  for (final bytes in [Uint8List(0), binary]) {
    final descriptor = await prepareUpload(connection, worktree, bytes);
    final upload = await connection.uploadAttachment(
      upload: jsonEncode(descriptor),
    );
    await fails(
      upload.write(bytes: Uint8List(chunkSize + 1)),
      'oversize rejected',
    );
    await write(upload, bytes);
    await upload.finish();
    await fails(
      upload.write(bytes: Uint8List(1)),
      'EOF rejects further writes',
    );
    publish = await connection.prepare(
      command: jsonEncode({
        'kind': 'finish_attachment_upload',
        'data': {'worktree': worktree, 'stream': descriptor['stream']},
      }),
    );
    published = await connection.execute(request: publish);
    saved = jsonDecode(published)['Ok']['data'];
    check(
      await connection.execute(request: publish) == published,
      'publication is idempotent',
    );
    await upload.close();
    upload.dispose();
    check(saved['spec']['name'] == '附件 🙂.bin', 'Unicode metadata retained');
    final received = await downloadAttachment(
      connection,
      await prepareDownload(connection, saved),
    );
    check(received.length == bytes.length, 'complete binary length');
    check(
      await revision(received) == saved['spec']['revision'],
      'complete binary digest',
    );
  }

  final digest = await Digest.newInstance();
  await fails(
    digest.update(bytes: Uint8List(chunkSize + 1)),
    'oversize hash chunk rejected',
  );
  check(
    await digest.revision() == await revision(Uint8List(0)),
    'failed update does not mutate digest',
  );
  digest.dispose();

  final small = Uint8List.fromList(utf8.encode('完整输入 🙂'));
  for (final corrupt in [false, true]) {
    final descriptor = await prepareUpload(connection, worktree, small);
    if (corrupt) descriptor['spec']['revision'] = List.filled(64, '0').join();
    final upload = await connection.uploadAttachment(
      upload: jsonEncode(descriptor),
    );
    await upload.write(
      bytes: corrupt ? small : Uint8List.sublistView(small, 1),
    );
    await fails(upload.finish(), 'short or corrupt upload cannot finish');
    await fails(upload.finish(), 'failed transfer stays failed');
    final finish = await connection.prepare(
      command: jsonEncode({
        'kind': 'finish_attachment_upload',
        'data': {'worktree': worktree, 'stream': descriptor['stream']},
      }),
    );
    check(
      jsonDecode(await connection.execute(request: finish))['Err'] != null,
      'failed upload is not published',
    );
    await upload.close();
    upload.dispose();
  }
  final changed = await prepareDownload(connection, saved);
  changed['attachment']['spec']['revision'] = List.filled(64, '0').join();
  await fails(
    downloadAttachment(connection, changed),
    'full bytes with wrong digest are not accepted',
  );

  final stalled = await connection.downloadAttachment(
    download: jsonEncode(await prepareDownload(connection, saved)),
  );
  check(
    (await execute(connection, 'read_attachment', {
          'worktree': worktree,
          'attachment': saved['id'],
        }))['data']['id'] ==
        saved['id'],
    'control commands respond during backpressure',
  );
  await stalled.close();
  await fails(stalled.next(), 'closed download fails');
  stalled.dispose();

  final unfinished = await prepareUpload(connection, worktree, binary);
  final upload = await connection.uploadAttachment(
    upload: jsonEncode(unfinished),
  );
  await upload.write(bytes: Uint8List.sublistView(binary, 0, chunkSize));
  final download = await connection.downloadAttachment(
    download: jsonEncode(await prepareDownload(connection, saved)),
  );
  check(
    (await download.next())!.isNotEmpty,
    'download begins before disconnect',
  );
  await controller.close();
  await fails(upload.write(bytes: small), 'controller close cancels upload');
  await fails(
    upload.finish(),
    'controller close cannot publish incomplete source',
  );
  await fails(download.next(), 'controller close cancels download');
  await fails(
    connection.uploadAttachment(upload: jsonEncode(unfinished)),
    'closed connection rejects transfer',
  );
  upload.dispose();
  download.dispose();
  connection.dispose();
  controller.dispose();

  controller = await Controller.open(path: path, internet: false, relays: []);
  connection = await controller.connect(address: address);
  check(
    await connection.execute(request: publish) == published,
    'publication receipt survives controller reopen',
  );
  final restored = await downloadAttachment(
    connection,
    await prepareDownload(connection, saved),
  );
  check(
    await revision(restored) == saved['spec']['revision'],
    'fresh transfer after reconnect verifies saved bytes',
  );
  for (var index = 0; index < 10; index++) {
    final descriptor = await prepareUpload(connection, worktree, small);
    final abandoned = await connection.uploadAttachment(
      upload: jsonEncode(descriptor),
    );
    abandoned.dispose();
  }
  final last = await uploadAttachment(
    connection,
    worktree,
    small,
    name: 'last.txt',
    mediaType: 'text/plain',
  );
  check(
    (await downloadAttachment(
          connection,
          await prepareDownload(connection, last),
        )).length ==
        small.length,
    'disposed handles release transfer capacity',
  );
  await connection.close();
  connection.dispose();
  await controller.close();
  controller.dispose();
  RustLib.dispose();
  print(
    'Dart FFI attachment bytes, verification, cancellation and reconnect passed',
  );
}
