import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:device_info_plus/device_info_plus.dart';
import 'package:path_provider/path_provider.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/bridge.dart';

import 'json.dart';
import 'background.dart';
import 'speech.dart';
import 'ports.dart';

typedef CommandHandler =
    Future<Map<String, dynamic>> Function(
      String kind,
      Map<String, dynamic>? data,
    );

/// Presentation lifetime only. Rust owns trust, subscriptions and projections.
class AppSession extends ChangeNotifier with WidgetsBindingObserver {
  AppSession()
    : _initialize = initializeBridge,
      _openController = _open,
      _createSpeech = SpeechController.new;

  @visibleForTesting
  AppSession.test({
    List<HostConnection> hosts = const [],
    this.controller,
    this.ready = true,
    Future<void> Function()? initialize,
    Future<Controller> Function(String, bool)? openController,
    SpeechController Function(String)? createSpeech,
  }) : _initialize = initialize ?? initializeBridge,
       _openController = openController ?? _open,
       _createSpeech = createSpeech ?? SpeechController.new {
    _hosts.addAll(hosts);
    _selected = hosts.firstOrNull?.id;
    for (final host in _hosts) {
      host.addListener(_changed);
    }
  }

  final Future<void> Function() _initialize;
  final Future<Controller> Function(String, bool) _openController;
  final SpeechController Function(String) _createSpeech;
  final background = BackgroundConnection();
  static Future<Controller> _open(String path, bool internet) async {
    final owner = await Controller.open(
      path: path,
      internet: internet,
      relays: const [],
    );
    try {
      final info = DeviceInfoPlugin();
      String name;
      if (Platform.isAndroid) {
        final android = await info.androidInfo;
        name = android.name.trim().isEmpty ? android.model : android.name;
      } else if (Platform.isIOS) {
        name = (await info.iosInfo).name;
      } else {
        name = Platform.localHostname;
      }
      await owner.setName(name: name);
    } catch (error) {
      // A missing platform display name must not prevent reconnecting trusted peers.
      debugPrint('Device name unavailable: $error');
    }
    return owner;
  }

  Controller? controller;
  SpeechController? speech;
  String profilePath = '';
  bool ready = false;
  bool loading = false;
  Object? error;
  bool _closed = false;
  final List<HostConnection> _hosts = [];
  String? _selected;

  List<HostConnection> get hosts => List.unmodifiable(_hosts);
  HostConnection? get selectedHost => host(_selected);
  HostConnection? host(String? id) =>
      _hosts.where((value) => value.id == id).firstOrNull;

  static AppSession of(BuildContext context) => maybeOf(context)!;
  static AppSession? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<SessionScope>()?.notifier;

  Future<void> start({String? path, bool internet = true}) async {
    if (loading || ready || _closed) return;
    loading = true;
    error = null;
    _changed();
    try {
      await _initialize();
      if (_closed) return;
      await background.initialize();
      if (_closed) return;
      profilePath =
          path ??
          '${(await getApplicationSupportDirectory()).path}${Platform.pathSeparator}controller';
      if (_closed) return;
      final opened = await _openController(profilePath, internet);
      if (_closed) {
        try {
          await opened.close();
        } finally {
          opened.dispose();
        }
        return;
      }
      controller = opened;
      speech = _createSpeech(profilePath);
      unawaited(speech!.check());
      final addresses = await opened.peers();
      if (_closed || controller != opened) return;
      for (final address in addresses) {
        await _connect(address, opened);
      }
      if (_closed || controller != opened) return;
      WidgetsBinding.instance.addObserver(this);
      ready = true;
    } catch (failure) {
      if (!_closed) error = failure;
      await _release();
    } finally {
      loading = false;
      _changed();
    }
  }

  Future<HostConnection> pair(String ticket) async {
    final owner = controller;
    if (_closed || !ready || owner == null) {
      throw StateError('controller is not ready');
    }
    final address = await owner.pair(ticket: ticket.trim());
    return _connect(address, owner);
  }

  Future<HostConnection> pairPin(String pin) async {
    final owner = controller;
    if (_closed || !ready || owner == null) {
      throw StateError('controller is not ready');
    }
    final address = await owner.pairPin(origin: '', pin: pin.trim());
    return _connect(address, owner);
  }

  Future<HostConnection> _connect(String address, Controller owner) async {
    if (_closed || controller != owner) {
      throw StateError('controller is closed');
    }
    final id = text(object(jsonDecode(address))['id']);
    if (id.isEmpty) throw const FormatException('endpoint has no identity');
    final existing = host(id);
    if (existing != null) {
      selectHost(id);
      return existing;
    }
    final connection = await owner.connect(address: address);
    if (_closed || controller != owner) {
      try {
        await connection.close();
      } finally {
        connection.dispose();
      }
      throw StateError('controller is closed');
    }
    final registered = host(id);
    if (registered != null) {
      try {
        await connection.close();
      } finally {
        connection.dispose();
      }
      selectHost(id);
      return registered;
    }
    final connected = HostConnection._(id, address, connection);
    connected.addListener(_changed);
    _hosts.add(connected);
    _selected ??= id;
    _changed();
    unawaited(connected.watch());
    return connected;
  }

  void selectHost(String id) {
    if (host(id) == null || _selected == id) return;
    _selected = id;
    _changed();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    final owner = controller;
    if (!_closed && state == AppLifecycleState.resumed && owner != null) {
      unawaited(
        owner.networkChanged().catchError((Object failure) {
          if (!_closed && controller == owner) {
            error = failure;
            _changed();
          }
        }),
      );
    }
  }

  void _changed() {
    background.update(hasHosts: !_closed && ready && _hosts.isNotEmpty);
    if (!_closed) notifyListeners();
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    WidgetsBinding.instance.removeObserver(this);
    ready = false;
    await background.close();
    await _release();
  }

  Future<void> _release() async {
    final hosts = List<HostConnection>.of(_hosts);
    _hosts.clear();
    _selected = null;
    for (final host in hosts) {
      host.removeListener(_changed);
    }
    speech?.dispose();
    speech = null;
    final opened = controller;
    controller = null;
    await Future.wait([
      for (final host in hosts)
        (() async {
          try {
            await host.close();
          } finally {
            host.dispose();
          }
        })(),
      if (opened != null)
        (() async {
          try {
            await opened.close();
          } finally {
            opened.dispose();
          }
        })(),
    ]).then<void>(
      (_) {},
      onError: (Object failure) {
        if (!_closed) error ??= failure;
      },
    );
  }

  @override
  void dispose() {
    unawaited(close());
    background.dispose();
    super.dispose();
  }
}

class SessionScope extends InheritedNotifier<AppSession> {
  const SessionScope({
    super.key,
    required AppSession session,
    required super.child,
  }) : super(notifier: session);
}

class HostConnection extends ChangeNotifier {
  HostConnection._(this.id, this.address, this._connection);

  @visibleForTesting
  // Public test arguments keep widget fixtures independent of native handles.
  // ignore: prefer_initializing_formals
  HostConnection.test({
    required this.id,
    required String this._label,
    this.snapshot = const {},
    this.connected = true,
    required CommandHandler command,
    this._connection,
    this.address = '',
  }) : _handler = command;

  PortMappings? _ports;
  PortMappings get ports => _ports ??= PortMappings(() => connection);

  final String id;
  final String address;
  final Connection? _connection;
  CommandHandler? _handler;
  Connection get connection =>
      _connection ?? (throw StateError('test connection has no native handle'));
  String? _label;
  String get label => _label ?? (id.length > 12 ? id.substring(0, 12) : id);
  Map<String, dynamic> snapshot = {};
  Map<String, dynamic> info = {};
  List<Map<String, dynamic>> notifications = [];
  bool connected = false;
  Object? error;
  Updates? _updates;
  bool _closed = false;
  bool _watching = false;

  Future<void> watch() async {
    if (_closed || _watching) return;
    _watching = true;
    Updates? owned;
    try {
      final updates = await connection.watch();
      if (_closed) {
        try {
          await updates.close();
        } finally {
          updates.dispose();
        }
        return;
      }
      owned = updates;
      _updates = updates;
      while (!_closed) {
        final view = object(jsonDecode(await updates.next()));
        if (_closed) break;
        snapshot = object(view['snapshot']);
        connected = view['connected'] == true;
        error = view['error'];
        notifications = objects(view['notifications']);
        notifyListeners();
        if (connected && info.isEmpty) unawaited(inspect());
      }
    } catch (failure) {
      if (!_closed) {
        connected = false;
        error = failure;
        notifyListeners();
      }
    } finally {
      _watching = false;
      if (owned != null && identical(_updates, owned)) {
        _updates = null;
        try {
          await owned.close();
        } finally {
          owned.dispose();
        }
      }
    }
  }

  bool _inspecting = false;
  Future<void> inspect() async {
    if (_inspecting) return;
    _inspecting = true;
    try {
      final response = await command('inspect_host');
      if (_closed) return;
      info = object(response['data']);
      _label = text(info['name'], label);
      notifyListeners();
    } catch (failure) {
      if (!_closed) error = failure;
    } finally {
      _inspecting = false;
    }
  }

  Future<Map<String, dynamic>> command(
    String kind, [
    Map<String, dynamic>? data,
  ]) async {
    if (_closed) throw StateError('connection is closed');
    if (_handler != null) return _handler!(kind, data);
    final request = await connection.prepare(
      command: jsonEncode({'kind': kind, 'data': ?data}),
    );
    return execute(request);
  }

  /// An explicit retry can reuse this request; no write is replayed automatically.
  Future<Map<String, dynamic>> execute(String request) async {
    if (_closed) throw StateError('connection is closed');
    Map<String, dynamic> result;
    try {
      result = object(jsonDecode(await connection.execute(request: request)));
    } catch (_) {
      throw CommandFailure('outcome_unknown', request: request);
    }
    if (result.containsKey('Err')) {
      final fault = object(result['Err']);
      throw CommandFailure(
        text(fault['code'], 'internal'),
        detail: text(fault['message']),
        request: request,
      );
    }
    if (!result.containsKey('Ok')) {
      throw CommandFailure('outcome_unknown', request: request);
    }
    return object(result['Ok']);
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    final updates = _updates;
    _updates = null;
    try {
      try {
        await updates?.close();
      } finally {
        updates?.dispose();
      }
    } finally {
      try {
        await _ports?.close();
      } finally {
        _ports?.dispose();
        try {
          await _connection?.close();
        } finally {
          _connection?.dispose();
        }
      }
    }
  }
}

class CommandFailure implements Exception {
  const CommandFailure(this.code, {this.detail = '', this.request});
  final String code;
  final String detail;
  // Retained only in memory, never logged, rendered or persisted in a UI cache.
  final String? request;
  @override
  String toString() => 'Command failed: $code';
}
