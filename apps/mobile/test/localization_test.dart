import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/conversations/message_composer.dart';
import 'package:sailry_mobile/features/conversations/live/tool_display.dart';
import 'package:sailry_mobile/features/conversations/live/tool_content.dart';
import 'package:sailry_mobile/features/settings/settings_page.dart';
import 'package:sailry_mobile/l10n/language.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

import 'notifications_test.dart' show Preferences;

class PendingPreferences extends Preferences {
  final loaded = Completer<String?>();

  @override
  Future<String?> getString(String key) =>
      key == 'language' ? loaded.future : super.getString(key);
}

Future<void> selectLanguage(WidgetTester tester, AppLanguage language) async {
  final context = tester.element(find.byType(SettingsPage));
  await tester.tap(find.widgetWithText(ListTile, context.tr('language')));
  await tester.pumpAndSettle();
  final choice = find
      .widgetWithText(ListTile, context.tr(language.labelKey))
      .last;
  await tester.ensureVisible(choice);
  await tester.tap(choice);
  await tester.pumpAndSettle();
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

  testWidgets('every language is selectable on a narrow screen', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(320, 640);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final session = AppSession.test();
    addTearDown(session.dispose);
    final preferences = Preferences();
    await tester.pumpWidget(
      SailryApp(session: session, preferences: preferences),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    final settings = tester.state(find.byType(SettingsPage));
    for (final language in AppLanguage.values.where(
      (value) => value != AppLanguage.system,
    )) {
      await selectLanguage(tester, language);
      expect(preferences.values['language'], language.name);
      final context = tester.element(find.byType(SettingsPage));
      expect(Localizations.localeOf(context), language.locale);
      expect(find.text(context.tr('settings')), findsWidgets);
      expect(MaterialLocalizations.of(context).cancelButtonLabel, isNotEmpty);
      expect(tester.state(find.byType(SettingsPage)), same(settings));
      expect(tester.takeException(), isNull, reason: language.name);
    }
  });

  testWidgets('plugin labels use controller locales and default names', (
    tester,
  ) async {
    final label = {
      'label': 'Inspect widget',
      'locales': {
        'zh-CN': '检查组件',
        'zh-TW': '檢查元件',
        'fr': 'Inspecter le composant',
        'pt-BR': 'Inspecionar componente',
      },
    };
    final cases = {
      AppLanguage.chinese.locale!: '检查组件',
      AppLanguage.traditionalChinese.locale!: '檢查元件',
      AppLanguage.french.locale!: 'Inspecter le composant',
      AppLanguage.portugueseBrazil.locale!: 'Inspecionar componente',
      AppLanguage.japanese.locale!: 'Inspect widget',
    };
    for (final entry in cases.entries) {
      await tester.pumpWidget(
        MaterialApp(
          locale: entry.key,
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          home: Builder(
            builder: (context) => Text(capturedLabel(context, label)),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text(entry.value), findsOneWidget);
    }
    label['locales'] = {'zh-CN': '检查组件'};
    await tester.pumpWidget(
      MaterialApp(
        locale: AppLanguage.traditionalChinese.locale,
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        home: Builder(
          builder: (context) => Text(capturedLabel(context, label)),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Inspect widget'), findsOneWidget);
    expect(toolLabel('plugin_future_operation'), 'plugin_future_operation');
    expect(tester.takeException(), isNull);
  });

  testWidgets('language works unpaired, persists and restores', (tester) async {
    final session = AppSession.test();
    addTearDown(session.dispose);
    final preferences = Preferences();
    await tester.pumpWidget(
      SailryApp(session: session, preferences: preferences),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    final settings = tester.state(find.byType(SettingsPage));
    await selectLanguage(tester, AppLanguage.english);
    expect(preferences.values['language'], 'english');
    expect(find.text('Settings'), findsWidgets);
    expect(find.text('Appearance'), findsOneWidget);
    expect(tester.state(find.byType(SettingsPage)), same(settings));
    final context = tester.element(find.byType(SettingsPage));
    expect(Localizations.localeOf(context).languageCode, 'en');
    expect(MaterialLocalizations.of(context).cancelButtonLabel, 'Cancel');
    for (final index in [0, 1, 2]) {
      await tester.tap(find.byKey(ValueKey('tab-$index')));
      await tester.pumpAndSettle();
      expect(find.text('Connect a host'), findsOneWidget);
    }
    expect(session.hosts, isEmpty);
    await tester.pumpWidget(const SizedBox());
    await tester.pumpWidget(
      SailryApp(session: session, preferences: preferences),
    );
    await tester.pumpAndSettle();
    expect(find.text('Connect a host'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    await selectLanguage(tester, AppLanguage.chinese);
    expect(preferences.values['language'], 'chinese');
    expect(find.text(tr('appearance')), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('system language follows locale changes and resolves regions', (
    tester,
  ) async {
    final session = AppSession.test();
    addTearDown(session.dispose);
    final preferences = Preferences()..values['language'] = 'system';
    tester.binding.platformDispatcher.localesTestValue = const [
      Locale('en', 'GB'),
    ];
    addTearDown(tester.binding.platformDispatcher.clearLocalesTestValue);
    await tester.pumpWidget(
      SailryApp(session: session, preferences: preferences),
    );
    await tester.pumpAndSettle();
    expect(find.text('Connect a host'), findsOneWidget);
    tester.binding.platformDispatcher.localesTestValue = const [
      Locale('zh', 'TW'),
    ];
    await tester.pumpAndSettle();
    final traditional = await AppLocalizations.delegate.load(
      AppLanguage.traditionalChinese.locale!,
    );
    expect(find.text(traditional.tr('hostConnectPrompt')), findsOneWidget);
    tester.binding.platformDispatcher.localesTestValue = const [Locale('fr')];
    await tester.pumpAndSettle();
    final french = await AppLocalizations.delegate.load(const Locale('fr'));
    expect(find.text(french.tr('hostConnectPrompt')), findsOneWidget);
    tester.binding.platformDispatcher.localesTestValue = const [Locale('ar')];
    await tester.pumpAndSettle();
    expect(find.text('Connect a host'), findsOneWidget);
    expect(tester.takeException(), isNull);
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

  testWidgets('late preference load preserves a language chosen in settings', (
    tester,
  ) async {
    final session = AppSession.test();
    addTearDown(session.dispose);
    final preferences = PendingPreferences();
    await tester.pumpWidget(
      SailryApp(session: session, preferences: preferences),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    await selectLanguage(tester, AppLanguage.english);
    preferences.loaded.complete('chinese');
    await tester.pumpAndSettle();
    expect(find.text('Appearance'), findsOneWidget);
    expect(preferences.values['language'], 'english');
    expect(tester.takeException(), isNull);
  });

  testWidgets('locale rebuild keeps drafts and translates the composer', (
    tester,
  ) async {
    final locale = ValueNotifier(const Locale('zh'));
    addTearDown(locale.dispose);
    final draft = TextEditingController(text: 'Keep my draft');
    addTearDown(draft.dispose);
    await tester.pumpWidget(
      ValueListenableBuilder(
        valueListenable: locale,
        builder: (context, value, _) => MaterialApp(
          locale: value,
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          home: Scaffold(
            body: MessageComposer(
              controller: draft,
              attached: false,
              busy: false,
              onAttachment: (_) {},
              onVoice: () {},
              onStop: () {},
              onSend: () {},
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    locale.value = const Locale('en');
    await tester.pumpAndSettle();
    expect(draft.text, 'Keep my draft');
    expect(find.byTooltip('Send'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
}
