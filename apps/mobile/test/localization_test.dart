import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/conversations/message_composer.dart';
import 'package:sailry_mobile/features/settings/settings_page.dart';
import 'package:sailry_mobile/l10n/language.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/kit.dart';
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
  await tester.tap(find.byType(SelectField<AppLanguage>));
  await tester.pumpAndSettle();
  await tester.tap(
    find.widgetWithText(MenuItemButton, context.tr(language.labelKey)),
  );
  await tester.pumpAndSettle();
}

void main() {
  test('ARB keys, placeholders and generated lookups agree', () async {
    final english =
        jsonDecode(File('lib/l10n/app_en.arb').readAsStringSync())
            as Map<String, dynamic>;
    final chinese =
        jsonDecode(File('lib/l10n/app_zh.arb').readAsStringSync())
            as Map<String, dynamic>;
    final keys = english.keys.where((key) => !key.startsWith('@')).toSet();
    expect(keys, chinese.keys.where((key) => !key.startsWith('@')).toSet());
    Set<String> placeholders(String value) => RegExp(
      r'\{(\w+)\}',
    ).allMatches(value).map((match) => match[1]!).toSet();
    for (final locale in AppLocalizations.supportedLocales) {
      final strings = await AppLocalizations.delegate.load(locale);
      final arb = locale.languageCode == 'en' ? english : chinese;
      for (final key in keys) {
        expect(
          strings.tr(key),
          arb[key],
          reason: '${locale.languageCode}: $key',
        );
        expect(
          placeholders(english[key] as String),
          placeholders(chinese[key] as String),
          reason: key,
        );
      }
      expect(() => strings.tr('missing-key'), throwsArgumentError);
    }
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
    expect(find.text(tr('hostConnectPrompt')), findsOneWidget);
    tester.binding.platformDispatcher.localesTestValue = const [Locale('fr')];
    await tester.pumpAndSettle();
    expect(find.text(tr('hostConnectPrompt')), findsOneWidget);
    expect(tester.takeException(), isNull);
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
