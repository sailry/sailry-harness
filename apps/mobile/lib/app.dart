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
import 'runtime/preferences.dart';
import 'l10n/strings.dart';
import 'l10n/language.dart';
import 'package:shared_preferences/shared_preferences.dart';

class SailryApp extends StatefulWidget {
  const SailryApp({super.key, this.session, this.preferences, this.updates});
  final AppSession? session;
  final SharedPreferencesAsync? preferences;
  final AppUpdates? updates;
  @override
  State<SailryApp> createState() => _SailryAppState();
}

class _SailryAppState extends State<SailryApp> {
  AppSession? _session;
  AppUpdates? _updates;
  final _navigator = GlobalKey<NavigatorState>();
  late final _preferences = AppPreferences(
    preferences: widget.preferences,
    persist: _persist,
    onError: _preferenceError,
  );
  bool get _persist => widget.session == null || widget.preferences != null;
  @override
  void initState() {
    super.initState();
    _updates = widget.updates ?? (widget.session == null ? AppUpdates() : null);
    _session = widget.session ?? AppSession();
    if (widget.session == null) unawaited(_session!.start());
    _preferences.addListener(_refreshPreferences);
    if (_persist) unawaited(_preferences.load());
  }

  void _refreshPreferences() {
    if (mounted) setState(() {});
  }

  void _preferenceError() {
    if (!mounted) return;
    final context = _navigator.currentContext;
    if (context != null) showToast(context, context.tr('preferencesFailed'));
  }

  @override
  void dispose() {
    _preferences.removeListener(_refreshPreferences);
    _preferences.dispose();
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
      themeMode: _preferences.mode,
      locale: _preferences.language.locale,
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
      language: _preferences.language,
      onLanguageChanged: _preferences.setLanguage,
      themeMode: _preferences.mode,
      onThemeChanged: _preferences.setTheme,
      notifications: _preferences.notifications,
      onNotificationsChanged: _preferences.setNotifications,
      updates: _updates,
    );
    if (_session != null) {
      home = NotificationDelivery(
        session: _session!,
        enabled:
            _preferences.notifications &&
            (!_persist ||
                _preferences.loaded ||
                _preferences.notificationsEdited),
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
                              _index = 3;
                            }),
                          ),
                          const ResourcesPage(),
                          SettingsPage(
                            updates: widget.updates,
                            language: widget.language,
                            onLanguageChanged: widget.onLanguageChanged,
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
