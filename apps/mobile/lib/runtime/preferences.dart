import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../l10n/language.dart';

class AppPreferences extends ChangeNotifier {
  AppPreferences({
    SharedPreferencesAsync? preferences,
    this.persist = true,
    this.onError,
  }) : _provided = preferences;

  final SharedPreferencesAsync? _provided;
  late final _preferences = _provided ?? SharedPreferencesAsync();
  final bool persist;
  final VoidCallback? onError;
  ThemeMode mode = ThemeMode.dark;
  AppLanguage language = AppLanguage.chinese;
  bool notifications = true;
  bool loaded = false;
  bool notificationsEdited = false;
  bool _themeEdited = false;
  bool _languageEdited = false;
  bool _closed = false;

  Future<void> load() async {
    if (!persist || _closed) return;
    try {
      final saved = await _preferences.getString('theme');
      final language = await _preferences.getString('language');
      final notifications =
          await _preferences.getBool('notifications.in_app') ?? true;
      if (_closed) return;
      if (!_themeEdited) {
        mode =
            ThemeMode.values.where((mode) => mode.name == saved).firstOrNull ??
            ThemeMode.dark;
      }
      if (!notificationsEdited) this.notifications = notifications;
      if (!_languageEdited) {
        this.language =
            AppLanguage.values
                .where((item) => item.name == language)
                .firstOrNull ??
            AppLanguage.chinese;
      }
      loaded = true;
      notifyListeners();
    } catch (_) {
      if (!_closed) onError?.call();
    }
  }

  Future<void> _save(Future<void> Function() save) async {
    if (!persist) return;
    try {
      await save();
    } catch (_) {
      if (!_closed) onError?.call();
    }
  }

  Future<void> setTheme(ThemeMode mode) async {
    if (_closed) return;
    _themeEdited = true;
    this.mode = mode;
    notifyListeners();
    await _save(() => _preferences.setString('theme', mode.name));
  }

  Future<void> setLanguage(AppLanguage language) async {
    if (_closed) return;
    _languageEdited = true;
    this.language = language;
    notifyListeners();
    await _save(() => _preferences.setString('language', language.name));
  }

  Future<void> setNotifications(bool enabled) async {
    if (_closed) return;
    notificationsEdited = true;
    notifications = enabled;
    notifyListeners();
    await _save(() => _preferences.setBool('notifications.in_app', enabled));
  }

  @override
  void dispose() {
    _closed = true;
    super.dispose();
  }
}
