import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/frb_generated.dart';
import 'conversation.dart' show check;

Future<void> released(
  int port, {
  Duration timeout = const Duration(seconds: 2),
}) async {
  final deadline = DateTime.now().add(timeout);
  final addressInUse = switch (Platform.operatingSystem) {
    'macos' || 'ios' => 48,
    'linux' || 'android' => 98,
    'windows' => 10048,
    _ => null,
  };
  while (true) {
    ServerSocket? listener;
    try {
      listener = await ServerSocket.bind(
        InternetAddress.loopbackIPv4,
        port,
        shared: false,
      );
    } on SocketException catch (error) {
      if (addressInUse == null || error.osError?.errorCode != addressInUse) {
        rethrow;
      }
    }
    if (listener != null) {
      await listener.close();
      return;
    }
    check(
      DateTime.now().isBefore(deadline),
      'forwarding listener was not released',
    );
    await Future<void>.delayed(const Duration(milliseconds: 20));
  }
}

Future<void> rejectsOccupied(int port) async {
  var rejected = false;
  try {
    await released(port, timeout: Duration.zero);
  } on StateError catch (error) {
    if (error.message != 'forwarding listener was not released') rethrow;
    rejected = true;
  }
  check(rejected, 'release probe rejects a live listener');
}

Future<void> main() async {
  final env = Platform.environment;
  final remotePort = int.parse(env['SAILRY_FORWARD_PORT']!);
  await rejectsOccupied(remotePort);
  await RustLib.init(
    externalLibrary: ExternalLibrary.open(env['SAILRY_BRIDGE_LIBRARY']!),
  );
  var controller = await Controller.open(
    path: env['SAILRY_CONTROLLER_PROFILE']!,
    internet: false,
    relays: [],
  );
  final address = await controller.pair(ticket: env['SAILRY_INVITATION']!);
  var connection = await controller.connect(address: address);
  for (final action in ['close', 'connection', 'controller', 'dispose']) {
    final forwarder = await connection.forwardPort(
      remotePort: remotePort,
      localPort: 0,
    );
    final port = await forwarder.localPort();
    check(
      jsonDecode(await forwarder.next())['kind'] == 'listening',
      'shared initial state reaches Dart',
    );
    final socket = await Socket.connect(InternetAddress.loopbackIPv4, port);
    final received = socket.fold<List<int>>(
      [],
      (bytes, chunk) => bytes..addAll(chunk),
    );
    final bytes = utf8.encode('Dart port $action');
    socket.add(bytes);
    await socket.flush();
    await socket.close();
    check(
      jsonEncode(await received.timeout(const Duration(seconds: 5))) ==
          jsonEncode(bytes),
      'forwarding preserves TCP half-close and response bytes',
    );
    socket.destroy();
    if (action == 'dispose') {
      forwarder.dispose();
    } else {
      if (action == 'close') await forwarder.close();
      if (action == 'connection') await connection.close();
      if (action == 'controller') await controller.close();
      final state = jsonDecode(
        await forwarder.next().timeout(const Duration(seconds: 5)),
      );
      check(
        state['kind'] != 'listening',
        'closing owner terminates forwarding',
      );
      await forwarder.close();
      forwarder.dispose();
    }
    await released(port);
    if (action == 'connection' || action == 'controller') {
      connection.dispose();
      if (action == 'controller') {
        controller.dispose();
        controller = await Controller.open(
          path: env['SAILRY_CONTROLLER_PROFILE']!,
          internet: false,
          relays: [],
        );
      }
      connection = await controller.connect(address: address);
    }
  }
  await controller.close();
  connection.dispose();
  controller.dispose();
  RustLib.dispose();
  stdout.writeln('Dart forwarding ownership and cleanup passed');
}
