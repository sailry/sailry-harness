import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';
import 'package:fluttertoast/fluttertoast.dart';
import 'l10n/strings.dart';
import 'package:sailry_mobile/l10n/language.dart';
import 'package:sailry_mobile/ui/app_background.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:sailry_mobile/features/settings/settings_page.dart';
import 'features/conversations/tasks_page.dart';
import 'features/resources/hosts_page.dart';
import 'features/resources/resources_page.dart';

/// Explicit presentation fixtures: no runtime, updates or preference persistence.
class PreviewApp extends StatefulWidget {
  const PreviewApp({super.key});
  @override
  State<PreviewApp> createState() => _PreviewAppState();
}

class _PreviewAppState extends State<PreviewApp> {
  ThemeMode _mode = ThemeMode.dark;
  AppLanguage _language = AppLanguage.chinese;
  bool _notifications = true;
  @override
  void dispose() {
    FToast().removeQueuedCustomToasts();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'Sailry Preview',
    debugShowCheckedModeBanner: false,
    theme: SailryTheme.of(Brightness.light),
    darkTheme: SailryTheme.of(Brightness.dark),
    themeMode: _mode,
    locale: _language.locale,
    localeListResolutionCallback: AppLanguage.resolve,
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    builder: (context, child) => FToastBuilder()(
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
    ),
    home: _Shell(
      language: _language,
      onLanguageChanged: (value) => setState(() => _language = value),
      themeMode: _mode,
      onThemeChanged: (value) => setState(() => _mode = value),
      notifications: _notifications,
      onNotificationsChanged: (value) => setState(() => _notifications = value),
    ),
  );
}

class _Shell extends StatefulWidget {
  const _Shell({
    required this.language,
    required this.onLanguageChanged,
    required this.themeMode,
    required this.onThemeChanged,
    required this.notifications,
    required this.onNotificationsChanged,
  });
  final AppLanguage language;
  final ValueChanged<AppLanguage> onLanguageChanged;
  final ThemeMode themeMode;
  final ValueChanged<ThemeMode> onThemeChanged;
  final bool notifications;
  final ValueChanged<bool> onNotificationsChanged;
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
    return Scaffold(
      extendBody: true,
      backgroundColor: Colors.transparent,
      body: SlidableAutoCloseBehavior(
        child: IndexedStack(
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
                        _taskFilter = kind == 'terminals' ? 'terminal' : 'all';
                        _sessionsOnly = kind == 'sessions';
                        _index = 0;
                      }),
                      onSettings: (host) => setState(() {
                        _index = 3;
                      }),
                    ),
                    const ResourcesPage(),
                    SettingsPage(
                      language: widget.language,
                      onLanguageChanged: widget.onLanguageChanged,
                      themeMode: widget.themeMode,
                      onThemeChanged: widget.onThemeChanged,
                      notifications: widget.notifications,
                      onNotificationsChanged: widget.onNotificationsChanged,
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
