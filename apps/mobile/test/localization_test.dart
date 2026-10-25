import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:shared_preferences_platform_interface/shared_preferences_async_platform_interface.dart';
import 'package:sailry_mobile/l10n/language.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/preferences.dart';

class Preferences extends Fake implements SharedPreferencesAsync {
  final values = <String, Object>{};
  Future<String?>? get pendingLanguage =>
      values['pendingLanguage'] as Future<String?>?;
  set pendingLanguage(Future<String?>? value) {
    if (value == null) {
      values.remove('pendingLanguage');
    } else {
      values['pendingLanguage'] = value;
    }
  }

  bool get fail => values['fail'] == true;
  set fail(bool value) => values['fail'] = value;

  @override
  Future<String?> getString(String key) async {
    if (fail) throw StateError('Preferences unavailable');
    if (key == 'language' && pendingLanguage != null) return pendingLanguage;
    return values[key] as String?;
  }

  @override
  Future<bool?> getBool(String key) async => values[key] as bool?;

  @override
  Future<void> setString(String key, String value) async {
    if (fail) throw StateError('Preferences unavailable');
    values[key] = value;
  }

  @override
  Future<void> setBool(String key, bool value) async {
    if (fail) throw StateError('Preferences unavailable');
    values[key] = value;
  }
}

void main() {
  test('ARB keys, placeholders and generated lookups agree', () async {
    final english =
        jsonDecode(File('lib/l10n/app_en.arb').readAsStringSync())
            as Map<String, dynamic>;
    final keys = english.keys.where((key) => !key.startsWith('@')).toSet();
    Set<String> placeholders(String value) => RegExp(
      r'\{(\w+)\}',
    ).allMatches(value).map((match) => match[1]!).toSet();
    for (final locale in AppLocalizations.supportedLocales) {
      final strings = await AppLocalizations.delegate.load(locale);
      final arb = {
        if (locale == const Locale('pt', 'BR'))
          ...jsonDecode(File('lib/l10n/app_pt.arb').readAsStringSync())
              as Map<String, dynamic>,
        ...jsonDecode(
              File('lib/l10n/app_${locale.toString()}.arb').readAsStringSync(),
            )
            as Map<String, dynamic>,
      };
      expect(keys, arb.keys.where((key) => !key.startsWith('@')).toSet());
      for (final key in keys) {
        expect(
          strings.tr(key),
          arb[key],
          reason: '${locale.languageCode}: $key',
        );
        expect(
          placeholders(english[key] as String),
          placeholders(arb[key] as String),
          reason: key,
        );
      }
      expect(() => strings.tr('missing-key'), throwsArgumentError);
    }
    expect(AppLanguage.values.length, 11);
    expect(AppLocalizations.supportedLocales.toSet(), {
      const Locale('pt'),
      ...AppLanguage.values.map((value) => value.locale).whereType<Locale>(),
    });
  });
  test('script, region and preference order resolve supported languages', () {
    final cases = <Locale, Locale>{
      const Locale('zh', 'HK'): AppLanguage.traditionalChinese.locale!,
      const Locale('zh', 'MO'): AppLanguage.traditionalChinese.locale!,
      const Locale.fromSubtags(
        languageCode: 'zh',
        scriptCode: 'Hans',
        countryCode: 'TW',
      ): AppLanguage.chinese.locale!,
      const Locale('ja', 'JP'): AppLanguage.japanese.locale!,
      const Locale('ko', 'KR'): AppLanguage.korean.locale!,
      const Locale('fr', 'CA'): AppLanguage.french.locale!,
      const Locale('de', 'DE'): AppLanguage.german.locale!,
      const Locale('es', 'MX'): AppLanguage.spanish.locale!,
      const Locale('pt', 'BR'): AppLanguage.portugueseBrazil.locale!,
      const Locale('ru', 'RU'): AppLanguage.russian.locale!,
      const Locale('pt', 'PT'): AppLanguage.english.locale!,
      const Locale('pt'): AppLanguage.english.locale!,
    };
    for (final entry in cases.entries) {
      expect(
        AppLanguage.resolve([entry.key], AppLocalizations.supportedLocales),
        entry.value,
      );
    }
    expect(
      AppLanguage.resolve(const [
        Locale('ar'),
        Locale('es'),
      ], AppLocalizations.supportedLocales),
      AppLanguage.spanish.locale,
    );
  });

  test('language choices persist and restore without pairing', () async {
    final storage = Preferences();
    final preferences = AppPreferences(preferences: storage);
    await preferences.load();
    expect(preferences.language, AppLanguage.chinese);
    for (final language in AppLanguage.values) {
      await preferences.setLanguage(language);
      expect(preferences.language, language);
      expect(storage.values['language'], language.name);
      final restored = AppPreferences(preferences: storage);
      await restored.load();
      expect(restored.language, language);
      restored.dispose();
    }
    preferences.dispose();
  });

  test('late preference reads preserve explicit choices', () async {
    final language = Completer<String?>();
    final storage = Preferences()
      ..values.addAll({'theme': 'dark', 'notifications.in_app': true})
      ..pendingLanguage = language.future;
    final preferences = AppPreferences(preferences: storage);
    final loading = preferences.load();
    await preferences.setTheme(ThemeMode.light);
    await preferences.setLanguage(AppLanguage.english);
    await preferences.setNotifications(false);
    language.complete('chinese');
    await loading;
    expect(preferences.mode, ThemeMode.light);
    expect(preferences.language, AppLanguage.english);
    expect(preferences.notifications, isFalse);
    expect(preferences.loaded, isTrue);
    expect(storage.values['language'], 'english');
    preferences.dispose();
  });

  test('saved appearance and notification choices restore', () async {
    final storage = Preferences()
      ..values.addAll({
        'theme': 'light',
        'language': 'system',
        'notifications.in_app': false,
      });
    final preferences = AppPreferences(preferences: storage);
    await preferences.load();
    expect(preferences.mode, ThemeMode.light);
    expect(preferences.language, AppLanguage.system);
    expect(preferences.notifications, isFalse);
    expect(preferences.loaded, isTrue);
    preferences.dispose();
  });

  test('disposal ignores late reads and preference failures', () async {
    final language = Completer<String?>();
    final storage = Preferences()..pendingLanguage = language.future;
    var notifications = 0;
    var errors = 0;
    final preferences = AppPreferences(
      preferences: storage,
      onError: () => errors++,
    )..addListener(() => notifications++);
    final loading = preferences.load();
    await Future<void>.value();
    preferences.dispose();
    language.completeError(StateError('Preferences unavailable'));
    await loading;
    expect(notifications, 0);
    expect(errors, 0);
  });

  test('preference failures retain choices and report once', () async {
    final storage = Preferences()..fail = true;
    var errors = 0;
    final preferences = AppPreferences(
      preferences: storage,
      onError: () => errors++,
    );
    await preferences.load();
    expect(preferences.loaded, isFalse);
    expect(errors, 1);
    await preferences.setLanguage(AppLanguage.english);
    expect(preferences.language, AppLanguage.english);
    expect(errors, 2);
    preferences.dispose();
  });

  test('preview preferences need no platform storage', () async {
    final previous = SharedPreferencesAsyncPlatform.instance;
    SharedPreferencesAsyncPlatform.instance = null;
    addTearDown(() => SharedPreferencesAsyncPlatform.instance = previous);
    final preferences = AppPreferences(persist: false);
    await preferences.load();
    expect(preferences.language, AppLanguage.chinese);
    await preferences.setLanguage(AppLanguage.english);
    await preferences.setTheme(ThemeMode.light);
    await preferences.setNotifications(false);
    expect(preferences.language, AppLanguage.english);
    expect(preferences.mode, ThemeMode.light);
    expect(preferences.notifications, isFalse);
    expect(SharedPreferencesAsyncPlatform.instance, isNull);
    preferences.dispose();
  });
}
