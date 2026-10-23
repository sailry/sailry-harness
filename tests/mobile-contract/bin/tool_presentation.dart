import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';

import 'conversation.dart' as conversation;

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
  conversation.Fixture? fixture;
  var restored = false;
  try {
    final project = await conversation.execute(connection, 'register_project', {
      'name': 'Mobile tool presentation',
      'path': env['SAILRY_PROJECT_PATH']!,
    });
    final snapshot = await conversation.execute(connection, 'snapshot', null);
    fixture = await conversation.create(
      connection,
      project['data']['id'],
      snapshot['data']['worktrees'][0]['id'],
    );
    await conversation.restore(connection, fixture);
    restored = true;
    stdout.writeln('Dart FFI shared tool visibility and output passed');
  } finally {
    if (!restored) await fixture?.server.close(force: true);
    await connection.close();
    connection.dispose();
    await controller.close();
    controller.dispose();
  }
}
