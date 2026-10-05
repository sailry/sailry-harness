import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';
import 'package:fluttertoast/fluttertoast.dart';
import 'features/conversations/tasks_page.dart';
import 'features/notifications/delivery.dart';
import 'features/resources/hosts_page.dart';
import 'features/resources/resources_page.dart';
import 'features/settings/settings_page.dart';
import 'features/updates/presentation.dart';
import 'features/updates/service.dart';
import 'ui/app_background.dart';
import 'ui/kit.dart';
import 'ui/theme.dart';
import 'ui/toast.dart';
import 'runtime/session.dart';
import 'l10n/strings.dart';
import 'l10n/language.dart';
import 'package:shared_preferences/shared_preferences.dart';

class SailryApp extends StatefulWidget {
  const SailryApp({
    super.key,
    this.session,
    this.preview = false,
    this.preferences,
    this.updates,
  });
  final AppSession? session;
  final bool preview;
  final SharedPreferencesAsync? preferences;
  final AppUpdates? updates;
  @override
  State<SailryApp> createState() => _SailryAppState();
}

class _SailryAppState extends State<SailryApp> {
  ThemeMode _mode = ThemeMode.dark;
  bool _themeEdited = false;
  AppLanguage _language = AppLanguage.chinese;
  bool _languageEdited = false;
  bool _notifications = true;
  bool _notificationsEdited = false;
  bool _preferencesLoaded = false;
  AppSession? _session;
  AppUpdates? _updates;
  final _navigator = GlobalKey<NavigatorState>();
  late final _preferences = widget.preferences ?? SharedPreferencesAsync();
  bool get _persist =>
      !widget.preview && (widget.session == null || widget.preferences != null);
  @override
  void initState() {
    super.initState();
    if (!widget.preview) {
      _updates =
          widget.updates ?? (widget.session == null ? AppUpdates() : null);
      _session = widget.session ?? AppSession();
      if (widget.session == null) unawaited(_session!.start());
      if (_persist) unawaited(_loadPreferences());
    }
  }

  Future<void> _loadPreferences() async {
    try {
      final saved = await _preferences.getString('theme');
      final language = await _preferences.getString('language');
      final notifications =
          await _preferences.getBool('notifications.in_app') ?? true;
      if (!mounted) return;
      setState(() {
        if (!_themeEdited) {
          _mode =
              ThemeMode.values
                  .where((mode) => mode.name == saved)
                  .firstOrNull ??
              ThemeMode.dark;
        }
        if (!_notificationsEdited) _notifications = notifications;
        if (!_languageEdited) {
          _language =
              AppLanguage.values
                  .where((item) => item.name == language)
                  .firstOrNull ??
              AppLanguage.chinese;
        }
        _preferencesLoaded = true;
      });
    } catch (_) {
      _preferenceError();
    }
  }

  void _preferenceError() {
    if (!mounted) return;
    final context = _navigator.currentContext;
    if (context != null) showToast(context, context.tr('preferencesFailed'));
  }

  Future<void> _savePreference(Future<void> Function() save) async {
    try {
      await save();
    } catch (_) {
      _preferenceError();
    }
  }

  void _setTheme(ThemeMode mode) {
    _themeEdited = true;
    setState(() => _mode = mode);
    if (_persist) {
      unawaited(
        _savePreference(() => _preferences.setString('theme', mode.name)),
      );
    }
  }

  void _setLanguage(AppLanguage language) {
    _languageEdited = true;
    setState(() => _language = language);
    if (_persist) {
      unawaited(
        _savePreference(
          () => _preferences.setString('language', language.name),
        ),
      );
    }
  }

  void _setNotifications(bool enabled) {
    _notificationsEdited = true;
    setState(() => _notifications = enabled);
    if (_persist) {
      unawaited(
        _savePreference(
          () => _preferences.setBool('notifications.in_app', enabled),
        ),
      );
    }
  }

  @override
  void dispose() {
    FToast().removeQueuedCustomToasts();
    if (widget.session == null) _session?.dispose();
    if (widget.updates == null) _updates?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final app = MaterialApp(
      title: 'Sailry',
      navigatorKey: _navigator,
      debugShowCheckedModeBanner: false,
      theme: SailryTheme.of(Brightness.light),
      darkTheme: SailryTheme.of(Brightness.dark),
      themeMode: _mode,
      locale: _language.locale,
      localeListResolutionCallback: AppLanguage.resolve,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      builder: (context, child) {
        _session?.background.localize(context.tr);
        return FToastBuilder()(
          context,
          ColoredBox(
            color: Theme.of(context).colorScheme.surface,
            child: Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 520),
                child: AppBackground(child: child!),
              ),
            ),
          ),
        );
      },
      home: _home(),
    );
    return _session == null
        ? app
        : SessionScope(session: _session!, child: app);
  }

  Widget _home() {
    Widget home = _Shell(
      language: _language,
      onLanguageChanged: _setLanguage,
      themeMode: _mode,
      onThemeChanged: _setTheme,
      notifications: _notifications,
      onNotificationsChanged: _setNotifications,
      updates: _updates,
    );
    if (_session != null) {
      home = NotificationDelivery(
        session: _session!,
        enabled:
            _notifications &&
            (!_persist || _preferencesLoaded || _notificationsEdited),
        child: home,
      );
    }
    if (_updates != null) {
      home = UpdateDelivery(updates: _updates!, child: home);
    }
    return home;
  }
}

class _Shell extends StatefulWidget {
  const _Shell({
    required this.language,
    required this.onLanguageChanged,
    required this.themeMode,
    required this.onThemeChanged,
    required this.notifications,
    required this.onNotificationsChanged,
    required this.updates,
  });
  final AppLanguage language;
  final ValueChanged<AppLanguage> onLanguageChanged;
  final ThemeMode themeMode;
  final ValueChanged<ThemeMode> onThemeChanged;
  final bool notifications;
  final ValueChanged<bool> onNotificationsChanged;
  final AppUpdates? updates;
  @override
  State<_Shell> createState() => _ShellState();
}

class _ShellState extends State<_Shell> {
  int _index = 0;
  int _taskRoute = 0;
  String _taskFilter = 'all';
  bool _sessionsOnly = false;
  String _settingsHost = 'Studio';
  @override
  Widget build(BuildContext context) {
    final session = AppSession.maybeOf(context);
    return Scaffold(
      extendBody: true,
      backgroundColor: Colors.transparent,
      body: SlidableAutoCloseBehavior(
        child: session != null && !session.ready && session.error != null
            ? PageFrame(
                title: context.tr('brand'),
                failure: FailureState(
                  message: context.tr('startupFailed'),
                  onRetry: session.start,
                ),
                child: const SizedBox.shrink(),
              )
            : IndexedStack(
                index: _index,
                children:
                    <Widget>[
                          TasksPage(
                            key: ValueKey(_taskRoute),
                            initialFilter: _taskFilter,
                            sessionsOnly: _sessionsOnly,
                          ),
                          HostsPage(
                            onBrowse: (kind) => setState(() {
                              _taskRoute++;
                              _taskFilter = kind == 'terminals'
                                  ? 'terminal'
                                  : 'all';
                              _sessionsOnly = kind == 'sessions';
                              _index = 0;
                            }),
                            onSettings: (host) => setState(() {
                              _settingsHost = host;
                              _index = 3;
                            }),
                          ),
                          const ResourcesPage(),
                          SettingsPage(
                            updates: widget.updates,
                            language: widget.language,
                            onLanguageChanged: widget.onLanguageChanged,
                            initialHost: _settingsHost,
                            onHostChanged: (host) =>
                                setState(() => _settingsHost = host),
                            themeMode: widget.themeMode,
                            onThemeChanged: widget.onThemeChanged,
                            notifications: widget.notifications,
                            onNotificationsChanged:
                                widget.onNotificationsChanged,
                          ),
                        ].indexed
                        .map(
                          (entry) => TickerMode(
                            enabled: entry.$1 == _index,
                            child: entry.$2,
                          ),
                        )
                        .toList(),
              ),
      ),
      bottomNavigationBar: FloatingNavigation(
        selected: _index,
        onSelected: (index) => setState(() {
          if (index == 0 && (_taskFilter != 'all' || _sessionsOnly)) {
            _taskRoute++;
            _taskFilter = 'all';
            _sessionsOnly = false;
          }
          _index = index;
        }),
      ),
    );
  }
}
