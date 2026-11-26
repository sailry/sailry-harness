import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:fluttertoast/fluttertoast.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'features/notifications/delivery.dart';
import 'features/resources/contributions.dart';
import 'features/updates/presentation.dart';
import 'features/updates/service.dart';
import 'l10n/language.dart';
import 'l10n/strings.dart';
import 'runtime/preferences.dart';
import 'runtime/session.dart';
import 'shell/assembly.dart';
import 'shell/shell.dart';
import 'ui/app_background.dart';
import 'ui/icons.dart';
import 'ui/theme.dart';
import 'ui/toast.dart';

void runSailryApp(MobileAssembly assembly, {AssetBundle? assets}) {
  WidgetsFlutterBinding.ensureInitialized();
  registerIconLicense(assets: assets);
  runApp(SailryApp(assembly: assembly, assets: assets));
}

class SailryApp extends StatefulWidget {
  const SailryApp({
    super.key,
    required this.assembly,
    this.session,
    this.preferences,
    this.updates,
    this.assets,
  });

  final MobileAssembly assembly;
  final AppSession? session;
  final SharedPreferencesAsync? preferences;
  final AppUpdates? updates;
  final AssetBundle? assets;

  @override
  State<SailryApp> createState() => _SailryAppState();
}

class _SailryAppState extends State<SailryApp> {
  late final AppSession _session;
  late final AppUpdates? _updates;
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
    _updates =
        widget.updates ??
        (widget.session == null ? widget.assembly.createUpdates?.call() : null);
    _session = widget.session ?? AppSession();
    if (widget.session == null) unawaited(_session.start());
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
    if (widget.session == null) _session.dispose();
    if (widget.updates == null) _updates?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final app = MaterialApp(
      title: 'Sailry',
      navigatorKey: _navigator,
      debugShowCheckedModeBanner: false,
      theme: SailryTheme.of(Brightness.light, _preferences.brand),
      darkTheme: SailryTheme.of(Brightness.dark, _preferences.brand),
      themeMode: _preferences.mode,
      locale: _preferences.language.locale,
      localeListResolutionCallback: AppLanguage.resolve,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      builder: (context, navigator) => FToastBuilder()(
        context,
        // The toast overlay retains its initial child. Observe the session
        // inside that subtree so pairing updates the mounted application frame.
        ListenableBuilder(
          listenable: _session,
          child: navigator,
          builder: (context, child) {
            _session.background.localize(context.tr);
            _session.localize(context.tr);
            return ColoredBox(
              color: Theme.of(context).colorScheme.surface,
              child: Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 520),
                  child: AppBackground(child: child!),
                ),
              ),
            );
          },
        ),
      ),
      home: _home(),
    );
    return DefaultAssetBundle(
      bundle: widget.assets ?? rootBundle,
      child: MobileAssemblyScope(
        assembly: widget.assembly,
        child: ProjectResourcesScope.registered(
          resources: widget.assembly.projectResources,
          child: SessionScope(session: _session, child: app),
        ),
      ),
    );
  }

  Widget _home() {
    final home = NotificationDelivery(
      session: _session,
      enabled:
          _preferences.notifications &&
          (!_persist ||
              _preferences.loaded ||
              _preferences.notificationsEdited),
      child: MobileShell(
        assembly: widget.assembly,
        preferences: _preferences,
        updates: _updates,
      ),
    );
    return _updates == null
        ? home
        : UpdateDelivery(updates: _updates, child: home);
  }
}
