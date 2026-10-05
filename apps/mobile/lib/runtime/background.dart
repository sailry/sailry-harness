import 'dart:async';
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:flutter_foreground_task/flutter_foreground_task.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../l10n/strings.dart';

/// Android process lifetime only. The existing Rust controller owns every link.
class BackgroundConnection extends ChangeNotifier with WidgetsBindingObserver {
  BackgroundConnection({bool? supported, SharedPreferencesAsync? preferences})
    : supported = supported ?? Platform.isAndroid,
      _providedPreferences = preferences;

  final bool supported;
  final SharedPreferencesAsync? _providedPreferences;
  late final _preferences = _providedPreferences ?? SharedPreferencesAsync();
  bool enabled = true;
  bool running = false;
  Object? error;
  bool _ready = false;
  bool _closed = false;
  bool _hasHosts = false;
  bool _permissionRequested = false;
  Future<void> _pending = Future.value();
  Translator _translate = tr;
  bool _notificationDirty = false;

  void localize(Translator translate) {
    if (_closed) return;
    final changed =
        _translate('backgroundConnectionActive') !=
        translate('backgroundConnectionActive');
    _translate = translate;
    if (changed) {
      _notificationDirty = true;
      if (_ready) unawaited(_sync());
    }
  }

  bool get _foreground =>
      WidgetsBinding.instance.lifecycleState == null ||
      WidgetsBinding.instance.lifecycleState == AppLifecycleState.resumed;

  Future<void> initialize() async {
    if (!supported || _ready || _closed) return;
    try {
      enabled = await _preferences.getBool('connection.background') ?? true;
      if (_closed) return;
      FlutterForegroundTask.init(
        androidNotificationOptions: AndroidNotificationOptions(
          channelId: 'host_connection',
          channelName: _translate('backgroundConnection'),
          channelImportance: NotificationChannelImportance.LOW,
          priority: NotificationPriority.LOW,
          onlyAlertOnce: true,
        ),
        iosNotificationOptions: const IOSNotificationOptions(),
        foregroundTaskOptions: ForegroundTaskOptions(
          eventAction: ForegroundTaskEventAction.nothing(),
          allowWakeLock: true,
          allowWifiLock: true,
          allowAutoRestart: false,
          // Use the manifest flag. The plugin's Dart override also stops on
          // Activity pause in 11.0.3, which would defeat background connections.
        ),
      );
      WidgetsBinding.instance.addObserver(this);
      _ready = true;
      await _sync();
    } catch (failure) {
      error = failure;
    }
    if (!_closed) notifyListeners();
  }

  void update({required bool hasHosts}) {
    if (_hasHosts == hasHosts || _closed) return;
    _hasHosts = hasHosts;
    if (_ready) unawaited(_sync());
  }

  Future<void> setEnabled(bool value) async {
    if (!supported || _closed) return;
    await _preferences.setBool('connection.background', value);
    if (_closed) return;
    enabled = value;
    error = null;
    notifyListeners();
    if (!_ready) await initialize();
    await _sync();
    if (error case final failure?) throw failure;
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed && _ready && !_closed) {
      unawaited(_sync());
    }
  }

  Future<void> _sync() {
    // Serialize starts/stops and recheck the latest intent after each await.
    return _pending = _pending.then((_) async {
      if (!_ready) return;
      try {
        var active = await FlutterForegroundTask.isRunningService;
        final wanted = !_closed && enabled && _hasHosts;
        if (wanted && !active && _foreground) {
          if (!_permissionRequested) {
            _permissionRequested = true;
            if (await FlutterForegroundTask.checkNotificationPermission() !=
                NotificationPermission.granted) {
              await FlutterForegroundTask.requestNotificationPermission();
            }
          }
          if (_closed || !enabled || !_hasHosts || !_foreground) return;
          // No callback or new controller: keep the current process and Rust
          // runtime alive instead of opening another client in a worker isolate.
          final result = await FlutterForegroundTask.startService(
            serviceTypes: [ForegroundServiceTypes.connectedDevice],
            notificationTitle: _translate('brand'),
            notificationText: _translate('backgroundConnectionActive'),
          );
          if (result is ServiceRequestFailure) throw result.error;
          active = true;
          _notificationDirty = false;
        } else if (wanted && active && _notificationDirty) {
          final result = await FlutterForegroundTask.updateService(
            notificationTitle: _translate('brand'),
            notificationText: _translate('backgroundConnectionActive'),
          );
          if (result is ServiceRequestFailure) throw result.error;
          _notificationDirty = false;
        } else if (!wanted && active) {
          final result = await FlutterForegroundTask.stopService();
          if (result is ServiceRequestFailure) throw result.error;
          active = false;
        }
        running = active;
        error = null;
      } catch (failure) {
        error = failure;
      }
      if (!_closed) notifyListeners();
    });
  }

  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    WidgetsBinding.instance.removeObserver(this);
    if (_ready) await _sync();
  }

  @override
  void dispose() {
    unawaited(close());
    super.dispose();
  }
}
