import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:device_info_plus/device_info_plus.dart';
import 'package:path_provider/path_provider.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/bridge.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../demo/connection.dart';
import '../l10n/strings.dart';
import 'json.dart';
import 'background.dart';
import 'speech.dart';
import 'ports.dart';

typedef CommandHandler =
    Future<Map<String, dynamic>> Function(
      String kind,
      Map<String, dynamic>? data,
    );

String _addressRouteKey(Map<String, dynamic> address) {
  final target = text(address['id']);
  final entry = text(object(address['route'])['entry'], target);
  return entry == target ? target : '$entry/$target';
}

/// Presentation lifetime only. Client and Link own trust and projections.
class AppSession extends ChangeNotifier with WidgetsBindingObserver {
  AppSession()
    : _initialize = initializeBridge,
      _openController = _open,
      _createSpeech = SpeechController.new,
      _savedPeers = _hasSavedPeers,
      _demoPreferences = SharedPreferencesAsync();

  @visibleForTesting
  AppSession.test({
    List<HostConnection> hosts = const [],
    this.controller,
    this.ready = true,
    Future<void> Function()? initialize,
    Future<Controller> Function(String, bool)? openController,
    SpeechController Function(String)? createSpeech,
    Future<bool> Function(String)? savedPeers,
    SharedPreferencesAsync? preferences,
  }) : _initialize = initialize ?? initializeBridge,
       _openController = openController ?? _open,
       _createSpeech = createSpeech ?? SpeechController.new,
       _savedPeers = savedPeers ?? ((_) async => true),
       _demoPreferences = preferences {
    _hosts.addAll(hosts);
    _selected = hosts.firstOrNull?.id;
    for (final host in _hosts) {
      host.addListener(_changed);
    }
  }

  final Future<void> Function() _initialize;
  final Future<Controller> Function(String, bool) _openController;
  final SpeechController Function(String) _createSpeech;
  final Future<bool> Function(String) _savedPeers;
  final SharedPreferencesAsync? _demoPreferences;
  static const _demoEnabledKey = 'hosts.demo.enabled';
  static const _demoSelectedKey = 'hosts.demo.selected';
  Translator _translate = tr;
  bool _internet = true;
  Future<Controller>? _opening;
  Timer? _peerPoll;
  Future<void>? _peerRefresh;

  // This is only a startup hint. Link still validates and owns the peer record.
  static Future<bool> _hasSavedPeers(String path) async {
    final file = File('$path${Platform.pathSeparator}peers.json');
    if (!await file.exists()) return false;
    try {
      if (await file.length() > 1024 * 1024) return true;
      final record = object(jsonDecode(await file.readAsString()));
      final peers = record['peers'];
      return peers is! List ||
          peers.any((peer) => peer is! Map || peer['removed'] != true);
    } catch (_) {
      return true;
    }
  }

  void localize(Translator translate) {
    _translate = translate;
    for (final host in _hosts.where((host) => host.isDemo)) {
      host._label = translate('demoHost');
    }
  }

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
  final Map<HostConnection, Future<void>> _removals = {};
  Future<void> _hostChanges = Future.value();
  String? _selected;
  String? _project;
  String? _worktree;

  List<HostConnection> get hosts => List.unmodifiable(_hosts);
  HostConnection? get selectedHost => host(_selected);
  // null selects all projects on the current host; the empty string selects
  // unassigned conversations. Explicit detail routes retain their own owner.
  String? get selectedProjectId => _project;
  String? get selectedWorktreeId {
    if (_project == null || _project!.isEmpty) return null;
    final trees = objects(
      selectedHost?.snapshot['worktrees'],
    ).where((tree) => tree['project'] == _project);
    final tree =
        trees.where((tree) => tree['id'] == _worktree).firstOrNull ??
        trees.where((tree) => tree['main'] == true).firstOrNull ??
        trees.firstOrNull;
    return tree?['id'] as String?;
  }

  bool isRemovingHost(HostConnection target) => _removals.containsKey(target);
  // The saved demo stays selectable without entering the live resource catalog.
  List<HostConnection> get resourceHosts {
    final demo = selectedHost?.isDemo == true;
    return List.unmodifiable(_hosts.where((host) => host.isDemo == demo));
  }

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
      _internet = internet;
      profilePath =
          path ??
          '${(await getApplicationSupportDirectory()).path}${Platform.pathSeparator}controller';
      if (_closed) return;
      final demo = await _demoPreferences?.getBool(_demoEnabledKey) ?? false;
      final selected =
          await _demoPreferences?.getBool(_demoSelectedKey) ?? false;
      if (_closed) return;
      if (demo) {
        _addDemo();
        if (!selected) _selected = null;
      }
      // A local demonstration never opens Link or a controller profile. A fresh
      // installation can also reach pairing before any native transport starts.
      if (!(demo && selected) && await _savedPeers(profilePath)) {
        await _ensureController();
      }
      if (_closed) return;
      _selected ??= _hosts.firstOrNull?.id;
      WidgetsBinding.instance.addObserver(this);
      ready = true;
      unawaited(refreshHosts(refreshConnections: false));
    } catch (failure) {
      if (!_closed) error = failure;
      await _release();
    } finally {
      loading = false;
      _changed();
    }
  }

  Future<Controller> _ensureController() async {
    if (_closed) throw StateError('controller is closed');
    final current = controller;
    if (current != null) return current;
    return _opening ??= _startController().whenComplete(() => _opening = null);
  }

  Future<Controller> _startController() async {
    await _initialize();
    if (_closed) throw StateError('controller is closed');
    await background.initialize();
    if (_closed) throw StateError('controller is closed');
    final opened = await _openController(profilePath, _internet);
    var registered = false;
    try {
      if (_closed) {
        throw StateError('controller is closed');
      }
      controller = opened;
      registered = true;
      speech = _createSpeech(profilePath);
      unawaited(speech!.check());
      // Saved direct hosts are available without waiting for their networks.
      // Resource routes arrive through the normal background directory refresh.
      final addresses = await opened.savedPeers();
      if (_closed || controller != opened) {
        throw StateError('controller is closed');
      }
      for (final address in addresses) {
        await _connect(address, opened, select: false);
      }
      if (_closed || controller != opened) {
        throw StateError('controller is closed');
      }
      _peerPoll?.cancel();
      _peerPoll = Timer.periodic(const Duration(seconds: 10), (_) {
        unawaited(refreshHosts());
      });
      if (ready) unawaited(refreshHosts(refreshConnections: false));
      return opened;
    } catch (_) {
      if (controller == opened) {
        await _release();
      } else if (!registered) {
        try {
          await opened.close();
        } finally {
          opened.dispose();
        }
      }
      rethrow;
    }
  }

  Future<HostConnection> pair(String ticket) async {
    if (_closed || !ready) {
      throw StateError('controller is not ready');
    }
    final owner = await _ensureController();
    return _change(owner, () async {
      final address = await owner.pair(ticket: ticket.trim());
      return _connect(address, owner, replaceDisconnected: true);
    });
  }

  Future<String> controllerIdentity(HostConnection host) async {
    if (host.isDemo) return jsonEncode(DemoConnection.controllerId);
    final owner = controller;
    if (owner == null) throw StateError('controller is not ready');
    return owner.id();
  }

  Future<HostConnection> pairPin(String pin) async {
    if (_closed || !ready) {
      throw StateError('controller is not ready');
    }
    // Reserved locally before opening a controller or resolving any pairing code.
    if (pin.trim() == DemoConnection.pairingCode) {
      return _change(controller, () async {
        await _demoPreferences?.setBool(_demoEnabledKey, true);
        await _demoPreferences?.setBool(_demoSelectedKey, true);
        if (_closed) throw StateError('controller is closed');
        final demo = _addDemo();
        selectHost(demo.id);
        return demo;
      });
    }
    final owner = await _ensureController();
    return _change(owner, () async {
      final address = await owner.pairPin(origin: '', pin: pin.trim());
      return _connect(address, owner, replaceDisconnected: true);
    });
  }

  HostConnection _addDemo() {
    final existing = host(DemoConnection.hostId);
    if (existing != null) return existing;
    final connection = DemoConnection(translate: (key) => _translate(key));
    final demo = HostConnection._(DemoConnection.hostId, '', connection)
      .._label = _translate('demoHost')
      ..snapshot = connection.snapshot
      ..info = connection.info
      ..connected = true;
    demo.addListener(_changed);
    _hosts.add(demo);
    _selected ??= demo.id;
    unawaited(demo.watch());
    _changed();
    return demo;
  }

  Future<T> _change<T>(Controller? owner, Future<T> Function() action) {
    // Pairing reveals its peer only after persisting trust, so local pairing
    // changes must serialize before starting the native operation.
    final pending = _hostChanges.then((_) {
      if (_closed || !ready || controller != owner) {
        throw StateError('controller is not ready');
      }
      return action();
    });
    _hostChanges = pending.then<void>((_) {}, onError: (Object _) {});
    return pending;
  }

  /// Reconciles Link's authenticated directory without changing the selected
  /// host or forgetting source-only peers under a new local removal intent.
  Future<void> refreshHosts({bool refreshConnections = true}) {
    final owner = controller;
    if (_closed || !ready || owner == null) return Future.value();
    return _peerRefresh ??=
        (() async {
              // Remote directory reads must not hold up local removal. The
              // queue identity also invalidates reads spanning a pairing change.
              final barrier = _hostChanges;
              await barrier;
              if (_closed ||
                  controller != owner ||
                  !identical(_hostChanges, barrier)) {
                return;
              }
              final addresses = await owner.peers();
              if (_closed ||
                  controller != owner ||
                  !identical(_hostChanges, barrier)) {
                return;
              }
              await _change(owner, () async {
                final ids = <String>{};
                for (final address in addresses) {
                  final id = _addressRouteKey(object(jsonDecode(address)));
                  if (id.isEmpty) {
                    throw const FormatException('endpoint has no identity');
                  }
                  ids.add(id);
                }
                for (final target in List.of(_hosts)) {
                  if (!target.isDemo && !ids.contains(target.id)) {
                    await _remove(target, owner, forget: false);
                  }
                }
                for (final address in addresses) {
                  if (_closed || controller != owner) return;
                  final target = await _connect(address, owner, select: false);
                  if (refreshConnections && !target.connected) {
                    unawaited(target.refresh());
                  }
                }
              });
            })()
            .catchError((Object failure) {
              if (!_closed && controller == owner) {
                debugPrint('Host directory unavailable: $failure');
              }
            })
            .whenComplete(() => _peerRefresh = null);
  }

  Future<HostConnection> _connect(
    String address,
    Controller owner, {
    bool select = true,
    bool replaceDisconnected = false,
  }) async {
    if (_closed || controller != owner) {
      throw StateError('controller is closed');
    }
    final decoded = object(jsonDecode(address));
    final id = _addressRouteKey(decoded);
    if (id.isEmpty) throw const FormatException('endpoint has no identity');
    var existing = host(id);
    // Pairing can restore trust after the native directory released a
    // revoked Client. A new native connection must not reuse that handle.
    if (existing != null && replaceDisconnected && existing._nativeInvalid) {
      await _remove(existing, owner, forget: false);
      existing = null;
    }
    if (existing != null) {
      existing._applyPeerMetadata(decoded);
      if (select) selectHost(id);
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
      registered._applyPeerMetadata(decoded);
      try {
        await connection.close();
      } finally {
        connection.dispose();
      }
      if (select) selectHost(id);
      return registered;
    }
    final connected = HostConnection._(id, address, connection)
      .._applyPeerMetadata(decoded);
    connected.addListener(_changed);
    _hosts.add(connected);
    unawaited(connected.watch());
    if (select || _selected == null) {
      selectHost(id);
    } else {
      _changed();
    }
    return connected;
  }

  void selectHost(String id) {
    final target = host(id);
    if (target == null || isRemovingHost(target)) return;
    if (_selected == id) {
      if (target._connection != null) unawaited(target.watch());
      return;
    }
    _selectContext(target, null, null);
  }

  void selectProject(String hostId, String? projectId) {
    final target = host(hostId);
    if (target == null || isRemovingHost(target)) return;
    if (projectId != null &&
        projectId.isNotEmpty &&
        !objects(
          target.snapshot['projects'],
        ).any((project) => project['id'] == projectId)) {
      return;
    }
    _selectContext(
      target,
      projectId,
      _selected == hostId && _project == projectId ? _worktree : null,
    );
  }

  void selectWorkspace(String hostId, String worktreeId) {
    final target = host(hostId);
    if (target == null || isRemovingHost(target)) return;
    final tree = objects(
      target.snapshot['worktrees'],
    ).where((tree) => tree['id'] == worktreeId).firstOrNull;
    final project = tree?['project'];
    if (project is! String ||
        !objects(
          target.snapshot['projects'],
        ).any((item) => item['id'] == project)) {
      return;
    }
    _selectContext(target, project, worktreeId);
  }

  void _selectContext(
    HostConnection target,
    String? project,
    String? worktree,
  ) {
    if (target._connection != null) unawaited(target.watch());
    if (_selected == target.id &&
        _project == project &&
        _worktree == worktree) {
      return;
    }
    final changedHost = _selected != target.id;
    _selected = target.id;
    _project = project;
    _worktree = worktree;
    if (changedHost) {
      unawaited(
        _demoPreferences
            ?.setBool(_demoSelectedKey, target.isDemo)
            .catchError(
              (Object failure) =>
                  debugPrint('Host selection preference failed: $failure'),
            ),
      );
    }
    _changed();
  }

  Future<void> removeHost(HostConnection target) {
    final pending = _removals[target];
    if (pending != null) return pending;
    if (host(target.id) != target) return Future.value();
    final owner = controller;
    if (_closed || !ready || owner == null && !target.isDemo) {
      return Future.error(StateError('controller is not ready'));
    }
    final removal = _removals.putIfAbsent(
      target,
      () => _change(owner, () => _remove(target, owner)).whenComplete(() {
        _removals.remove(target);
        _changed();
      }),
    );
    _changed();
    return removal;
  }

  Future<void> _remove(
    HostConnection target,
    Controller? owner, {
    bool forget = true,
  }) async {
    if (host(target.id) != target) return;
    if (target.isDemo) {
      await _demoPreferences?.setBool(_demoEnabledKey, false);
      await _demoPreferences?.setBool(_demoSelectedKey, false);
    } else if (forget) {
      await owner!.forget(address: target.address);
    }
    if (_closed || controller != owner || host(target.id) != target) return;
    if (!target.routed) {
      for (final child in List.of(_hosts)) {
        if (child.routed && child.entryId == target.nodeId) {
          await _remove(child, owner, forget: false);
        }
      }
    }
    target.removeListener(_changed);
    _hosts.remove(target);
    if (_selected == target.id) {
      _selected = null;
      final next = _hosts.where((host) => !isRemovingHost(host)).firstOrNull;
      if (next != null) selectHost(next.id);
    }
    _changed();
    try {
      await target.close();
    } catch (failure) {
      // The saved pairing is already removed even if cleanup fails.
      debugPrint('Removed host cleanup failed: $failure');
    } finally {
      target.dispose();
    }
    if (target.isDemo && _hosts.isEmpty && await _savedPeers(profilePath)) {
      try {
        await _ensureController();
      } catch (failure) {
        if (!_closed) error = failure;
      }
      _changed();
    }
  }

  bool _away = false;

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.hidden) {
      _away = true;
      return;
    }
    final owner = controller;
    if (!_closed && state == AppLifecycleState.resumed && owner != null) {
      final returned = _away;
      _away = false;
      unawaited(
        owner
            .networkChanged()
            .catchError((Object failure) {
              if (!_closed && controller == owner) {
                error = failure;
                _changed();
              }
            })
            .whenComplete(() {
              unawaited(refreshHosts());
              // Views can be stale after a long background period.
              if (!returned || _closed || controller != owner) return;
              for (final host in List.of(_hosts)) {
                unawaited(host.refresh());
              }
            }),
      );
    }
  }

  void _changed() {
    if (selectedHost == null ||
        _project != null &&
            _project!.isNotEmpty &&
            !objects(
              selectedHost?.snapshot['projects'],
            ).any((project) => project['id'] == _project)) {
      _project = null;
      _worktree = null;
    }
    background.update(
      hasHosts: !_closed && ready && _hosts.any((host) => !host.isDemo),
    );
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
    _peerPoll?.cancel();
    _peerPoll = null;
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
  HostConnection.test({
    required this.id,
    required String this._label,
    this.snapshot = const {},
    this.childTargets = const {},
    this.connected = true,
    CommandHandler? command,
    this._connection,
    this.address = '',
    Map<String, dynamic> metadata = const {},
  }) : _handler = command {
    _applyPeerMetadata(metadata);
  }

  PortMappings? _ports;
  PortMappings get ports => _ports ??= PortMappings(() => connection);

  final String id;
  final String address;
  String get nodeId => id.split('/').last;
  String get entryId => id.split('/').first;
  bool get routed => entryId != nodeId;
  final Connection? _connection;
  bool get isDemo => _connection is DemoConnection;
  CommandHandler? _handler;
  Connection get connection =>
      _connection ?? (throw StateError('test connection has no native handle'));
  String? _label;
  String _directoryName = '';
  bool _sourceUnavailable = false;
  String _sourceFaultCode = '';
  bool _nativeInvalid = false;
  Map<String, dynamic> _directorySnapshot = {};
  num _directoryRevision = 0;
  bool direct = true;
  List<String> sources = const [];
  String get label =>
      _label ??
      (_directoryName.isNotEmpty
          ? _directoryName
          : (id.length > 12 ? id.substring(0, 12) : id));

  void _applyPeerMetadata(Map<String, dynamic> address) {
    final relationship = object(address['relationship']);
    final nextDirect = relationship.isEmpty || relationship['direct'] == true;
    final rawSources = relationship['sources'];
    final nextSources = rawSources is List
        ? (rawSources.whereType<String>().toList()..sort())
        : sources;
    final name = text(relationship['name'], _directoryName);
    final sourceFault = object(address['source_fault']);
    final directory = object(address['resource_source']);
    final directoryChanged =
        directory.isNotEmpty &&
        (_directorySnapshot.isEmpty ||
            _directoryRevision != number(directory['revision']));
    if (directory.isNotEmpty) {
      _directorySnapshot = {
        'projects': objects(directory['projects']),
        'worktrees': objects(directory['worktrees']),
      };
      _directoryRevision = number(directory['revision']);
    }
    final unavailable = sourceFault.isNotEmpty;
    final changed =
        directoryChanged ||
        direct != nextDirect ||
        sources.join('\u0000') != nextSources.join('\u0000') ||
        _directoryName != name ||
        _sourceUnavailable != unavailable ||
        _sourceFaultCode != text(sourceFault['code']);
    _sourceUnavailable = unavailable;
    _sourceFaultCode = text(sourceFault['code']);
    if (unavailable) {
      connected = false;
      error = sourceFault;
      if (const {
        'permission_denied',
        'not_found',
        'expired',
      }.contains(_sourceFaultCode)) {
        _nativeInvalid = true;
        _directorySnapshot = {};
        snapshot = {};
        notifications = [];
        childTargets = {};
      }
    }
    direct = nextDirect;
    sources = List.unmodifiable(nextSources);
    _directoryName = name;
    _applyDirectorySnapshot();
    if (changed && !_closed) notifyListeners();
  }

  void _applyDirectorySnapshot() {
    if (_directorySnapshot.isNotEmpty &&
        number(snapshot['cursor']) <= _directoryRevision) {
      snapshot = {...snapshot, ..._directorySnapshot};
    }
  }

  String originDescription(
    Translator translate,
    Iterable<HostConnection> hosts,
  ) {
    if (isDemo) return '';
    if (direct || sources.isEmpty) return translate('hostOriginDirect');
    final names = sources
        .map((source) {
          final known = hosts
              .where((host) => host.nodeId == source && !host.routed)
              .firstOrNull;
          return known?.label ??
              (source.length > 12 ? source.substring(0, 12) : source);
        })
        .join(', ');
    return translate('hostOriginSource').replaceAll('{name}', names);
  }

  Map<String, dynamic> snapshot = {};
  Map<String, dynamic> info = {};
  List<Map<String, dynamic>> notifications = [];
  Map<String, dynamic> childTargets = {};
  bool connected = false;
  Map<String, dynamic> transport = {};
  Object? error;
  Updates? _updates;
  bool _closed = false;
  bool _watching = false;
  bool _restarting = false;
  Completer<void>? _fresh;

  /// Resubscribes for a fresh snapshot; the platform may stall the update
  /// stream while the app is in the background. Completes on the first view.
  Future<void> refresh() async {
    if (_closed || _connection == null) return;
    final fresh = _fresh ??= Completer<void>();
    final updates = _updates;
    if (updates != null) {
      _restarting = true;
      unawaited(updates.close());
    } else if (!_watching) {
      unawaited(watch());
    }
    await fresh.future.timeout(const Duration(seconds: 10), onTimeout: () {});
  }

  void _settle() {
    final fresh = _fresh;
    _fresh = null;
    if (fresh != null && !fresh.isCompleted) fresh.complete();
  }

  Future<void> watch() async {
    if (_closed || _watching) return;
    _watching = true;
    Updates? owned;
    Object? observedError;
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
        connected = view['connected'] == true && !_sourceUnavailable;
        transport = connected ? object(view['connection']) : {};
        final fault = view['error'];
        observedError = fault is Map ? fault : null;
        error = fault;
        notifications = objects(view['notifications']);
        childTargets = object(view['child_targets']);
        final code = text(object(fault)['code'], _sourceFaultCode);
        if (const {
          'permission_denied',
          'not_found',
          'expired',
        }.contains(code)) {
          _nativeInvalid = true;
          _directorySnapshot = {};
          snapshot = {};
          notifications = [];
          childTargets = {};
        } else {
          _applyDirectorySnapshot();
        }
        notifyListeners();
        _settle();
        if (connected && info.isEmpty) unawaited(inspect());
      }
    } catch (failure) {
      if (!_closed && !_restarting) {
        if (failure is BridgeFailure &&
            const {
              'cancelled',
              'permission_denied',
              'not_found',
              'expired',
            }.contains(failure.code)) {
          _nativeInvalid = true;
        }
        connected = false;
        transport = {};
        error = failure.toString() == 'channel closed' && observedError != null
            ? observedError
            : failure;
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
      if (_restarting) {
        _restarting = false;
        if (!_closed) unawaited(watch());
      } else {
        _settle();
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
      final name = text(info['name']);
      if (name.isNotEmpty) _label = name;
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
    if (!connected && kind != 'inspect_host') {
      throw CommandFailure('unavailable');
    }
    if (_handler != null) return _handler!(kind, data);
    final request = await connection.prepare(
      command: jsonEncode({'kind': kind, 'data': ?data}),
    );
    return execute(request);
  }

  /// Sends [request] once. A result that was not observed is looked up on
  /// the Node; no write is replayed automatically.
  Future<Map<String, dynamic>> execute(String request) =>
      _outcome(request, () => connection.execute(request: request));

  /// Asks the Node what became of a retained [request] without sending it
  /// again: its stored result, its stored failure, a [CommandFailure.released]
  /// failure when it was never applied or can never be confirmed, or
  /// `outcome_unknown` while it still runs or the Node cannot be asked.
  Future<Map<String, dynamic>> check(String request) =>
      _outcome(request, () => connection.resolve(request: request));

  Future<Map<String, dynamic>> _outcome(
    String request,
    Future<String> Function() send,
  ) async {
    if (_closed) throw StateError('connection is closed');
    Map<String, dynamic> result;
    try {
      result = object(jsonDecode(await send()));
    } on BridgeFailure catch (failure) {
      throw CommandFailure(
        failure.code,
        detail: failure.message,
        request: request,
      );
    } catch (_) {
      throw CommandFailure('outcome_unknown', request: request);
    }
    if (result.containsKey('Err')) {
      final fault = object(result['Err']);
      throw CommandFailure(
        text(fault['code'], 'internal'),
        detail: text(fault['message']),
        details: object(fault['details']),
        request: request,
        released: result['released'] == true,
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
    connected = false;
    transport = {};
    notifyListeners();
    _settle();
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

/// Localization key naming the side to update after a protocol mismatch.
/// Accepts a [CommandFailure] or a serialized fault from a Client view.
String? versionMismatchKey(Object? failure) {
  final details = failure is CommandFailure
      ? failure.details
      : failure is Map
      ? failure['details']
      : null;
  if (details is! Map || details['kind'] != 'version_mismatch') return null;
  return details['update_host'] == true
      ? 'hostUpdateRequired'
      : 'appUpdateRequired';
}

class CommandFailure implements Exception {
  const CommandFailure(
    this.code, {
    this.detail = '',
    this.details = const {},
    this.request,
    this.released = false,
  });
  final String code;
  final String detail;
  // Structured fault details from the shared protocol, such as version_mismatch.
  final Map<String, dynamic> details;
  // Retained only in memory, never logged, rendered or persisted in a UI cache.
  final String? request;
  // No further inspection is needed: the Node reported a terminal failure,
  // the request never arrived, or its result can never be confirmed.
  // The request is dropped; the next attempt is a new one.
  final bool released;

  /// The result is still to be learned: keep [request] and check it later.
  bool get retained =>
      code == 'outcome_unknown' && request != null && !released;

  // Transport, cancellation and pairing failures do not resolve an older send.
  bool get rejected =>
      released ||
      switch (code) {
        'invalid_request' ||
        'revision_conflict' ||
        'conflict' ||
        'not_found' ||
        'not_configured' => true,
        _ => false,
      };
  @override
  String toString() => 'Command failed: $code';
}
