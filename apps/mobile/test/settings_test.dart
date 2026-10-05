import 'dart:async';
import 'dart:ui' show Tristate;
import 'dart:convert';
import 'package:fl_chart/fl_chart.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/settings/record_form.dart';
import 'package:sailry_mobile/features/settings/settings_page.dart';
import 'package:sailry_mobile/features/settings/usage_data.dart';
import 'package:sailry_mobile/features/settings/usage_page.dart';
import 'package:sailry_mobile/features/settings/usage_watch.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/l10n/language.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

Future<void> pumpPage(
  WidgetTester tester,
  Widget page, {
  AppSession? session,
  double width = 390,
  Brightness brightness = Brightness.light,
}) async {
  tester.view.physicalSize = Size(width, 844);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  final owner = session ?? AppSession.test();
  addTearDown(owner.dispose);
  await tester.pumpWidget(
    SessionScope(
      session: owner,
      child: MaterialApp(theme: SailryTheme.of(brightness), home: page),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> tap(WidgetTester tester, Finder finder) async {
  await tester.ensureVisible(finder);
  await tester.pumpAndSettle();
  await tester.tap(finder);
  await tester.pumpAndSettle();
}

Future<void> pumpForm(
  WidgetTester tester,
  Widget form,
  NodeFixture node,
) async {
  await pumpPage(
    tester,
    Scaffold(
      body: Builder(
        builder: (context) => TextButton(
          onPressed: () => showAppSheet(context, tr('edit'), child: form),
          child: const Text('Open'),
        ),
      ),
    ),
    session: AppSession.test(hosts: [node.host]),
  );
  await tap(tester, find.text('Open'));
}

class NodeFixture {
  NodeFixture(this.id, this.label) {
    host = HostConnection.test(
      id: id,
      label: label,
      snapshot: {
        'projects': [
          {'id': '$id-project', 'name': '$label project'},
        ],
      },
      command: command,
    );
  }
  final String id;
  final String label;
  late final HostConnection host;
  final calls = <(String, Map<String, dynamic>?)>[];
  final roles = <Map<String, dynamic>>[];
  bool conflict = false;
  Map<String, dynamic> provider = {
    'id': 'provider',
    'revision': 3,
    'name': 'OpenAI',
    'api': 'chat_completions',
    'authentication': 'api_key',
    'endpoint': 'https://example.test/v1',
    'enabled': true,
    'models': [
      {
        'id': 'model',
        'context': 128000,
        'output': 16384,
        'vision': true,
        'tools': true,
        'reasoning': false,
        'web_search': false,
        'generates': [],
        'efforts': [],
        'custom_efforts': false,
        'default_effort': 'default',
      },
    ],
    'default_model': 'model',
    'credential': {'id': 'credential', 'revision': 2},
  };
  Future<Map<String, dynamic>> command(
    String kind,
    Map<String, dynamic>? data,
  ) async {
    calls.add((kind, data));
    switch (kind) {
      case 'list_roles':
        return {'kind': 'roles', 'data': roles};
      case 'put_role':
        if (conflict) throw const CommandFailure('revision_conflict');
        final role = Map<String, dynamic>.from(data!['role']);
        role['revision'] = (data['expected_revision'] as int) + 1;
        roles.removeWhere((item) => item['id'] == role['id']);
        roles.add(role);
        return {'kind': 'role', 'data': role};
      case 'remove_role':
        roles.removeWhere((item) => item['id'] == data!['role']);
        return {'kind': 'role_removed'};
      case 'list_providers':
        return {
          'kind': 'providers',
          'data': [provider],
        };
      case 'read_provider_key':
        return {'kind': 'provider_key', 'data': 'isolated-api-key'};
      case 'save_provider':
        if (conflict) throw const CommandFailure('revision_conflict');
        provider = Map<String, dynamic>.from(data!['provider']);
        return {'kind': 'provider', 'data': provider};
      case 'put_memory':
        return {'kind': 'memory', 'data': data!['entry']};
      default:
        throw StateError('Unexpected command: $kind');
    }
  }
}

void main() {
  for (final brightness in Brightness.values) {
    testWidgets('language choices use the appearance list in $brightness', (
      tester,
    ) async {
      final semantics = tester.ensureSemantics();
      try {
        AppLanguage? chosen;
        await pumpPage(
          tester,
          SettingsPage(
            themeMode: brightness == Brightness.dark
                ? ThemeMode.dark
                : ThemeMode.light,
            onThemeChanged: (_) {},
            language: AppLanguage.chinese,
            onLanguageChanged: (language) => chosen = language,
          ),
          brightness: brightness,
        );
        await tap(tester, find.text(tr('language')));
        expect(find.byType(SelectField<AppLanguage>), findsNothing);
        expect(find.byType(MenuAnchor), findsNothing);
        for (final language in AppLanguage.values) {
          final row = find.widgetWithText(ListTile, tr(language.labelKey)).last;
          final tile = tester.widget<ListTile>(row);
          expect(tile.onTap, isNotNull);
          expect(tile.selected, language == AppLanguage.chinese);
          expect(
            find.descendant(
              of: row,
              matching: find.byWidgetPredicate(
                (widget) => widget is AppIcon && widget.name == 'check',
              ),
            ),
            language == AppLanguage.chinese ? findsOneWidget : findsNothing,
          );
          expect(
            tester.getSemantics(row).flagsCollection.isSelected,
            language == AppLanguage.chinese
                ? Tristate.isTrue
                : Tristate.isFalse,
          );
        }
        await tap(
          tester,
          find.widgetWithText(ListTile, tr(AppLanguage.english.labelKey)),
        );
        expect(chosen, AppLanguage.english);
        expect(find.byType(BottomSheet), findsNothing);
        expect(tester.takeException(), isNull);
      } finally {
        semantics.dispose();
      }
    });
  }

  testWidgets('appearance stays available before pairing', (tester) async {
    ThemeMode? chosen;
    await pumpPage(
      tester,
      SettingsPage(
        themeMode: ThemeMode.light,
        onThemeChanged: (mode) => chosen = mode,
      ),
    );
    for (final label in ['providers', 'roles', 'memorySettings', 'usage']) {
      final row = tester.widget<ListTile>(
        find.widgetWithText(ListTile, tr(label)),
      );
      expect(row.enabled, isFalse);
      expect(row.onTap, isNull);
    }
    await tap(tester, find.text(tr('appearance')));
    await tap(tester, find.text(tr('dark')));
    expect(chosen, ThemeMode.dark);
    expect(tester.takeException(), isNull);
  });

  testWidgets('role save preserves its host and conflicting draft', (
    tester,
  ) async {
    final first = NodeFixture('first', 'Studio');
    final second = NodeFixture('second', 'Build Server');
    final session = AppSession.test(hosts: [first.host, second.host]);
    await pumpPage(
      tester,
      SettingsPage(themeMode: ThemeMode.light, onThemeChanged: (_) {}),
      session: session,
    );
    await tap(tester, find.text(tr('roles')));
    await tap(tester, find.widgetWithText(ListTile, tr('add')));
    for (final field in {
      'name': 'Designer',
      'key': 'designer',
      'description': 'Reviews layout',
      'content': 'Preserve accessible focus',
    }.entries) {
      await tester.enterText(
        find.byKey(ValueKey('settings-${field.key}')),
        field.value,
      );
    }
    session.selectHost('second');
    await tester.pump();
    first.conflict = true;
    await tap(tester, find.widgetWithText(FilledButton, tr('save')));
    expect(find.text(tr('settingsConflict')), findsOneWidget);
    expect(
      tester
          .widget<TextFormField>(find.byKey(const ValueKey('settings-name')))
          .controller!
          .text,
      'Designer',
    );
    expect(second.calls, isEmpty);
    first.conflict = false;
    await tap(tester, find.widgetWithText(FilledButton, tr('save')));
    expect(first.roles.single['name'], 'Designer');
    expect(second.roles, isEmpty);
    expect(first.calls.last.$1, 'list_roles');
    expect(tester.takeException(), isNull);
  });

  for (final (api, endpoint) in [
    ('open_code_go', 'https://opencode.ai/zen/go/v1'),
    ('open_code_zen', 'https://opencode.ai/zen/v1'),
  ]) {
    testWidgets('$api provider key read and clear excludes login tokens', (
      tester,
    ) async {
      final node = NodeFixture('node', 'Node');
      node.provider['api'] = api;
      await pumpPage(
        tester,
        Scaffold(
          body: SingleChildScrollView(
            child: RecordForm(
              host: node.host,
              kind: 'providers',
              record: node.provider,
            ),
          ),
        ),
        session: AppSession.test(hosts: [node.host]),
      );
      expect(find.byKey(const ValueKey('settings-endpoint')), findsNothing);
      final key = find.byKey(const ValueKey('settings-secret'));
      expect(
        tester.widget<TextFormField>(key).controller!.text,
        'isolated-api-key',
      );
      expect(
        tester
            .widget<EditableText>(
              find.descendant(of: key, matching: find.byType(EditableText)),
            )
            .obscureText,
        isFalse,
      );
      await tester.enterText(key, '');
      node.conflict = true;
      await tap(tester, find.widgetWithText(FilledButton, tr('save')));
      final saved = node.calls.last.$2!;
      expect(saved['expected_revision'], 3);
      expect(saved['secret'], isNull);
      expect(saved['provider']['credential'], isNull);
      expect(saved['provider']['models'].single['vision'], isTrue);
      expect(saved['provider']['api'], api);
      expect(saved['provider']['endpoint'], endpoint);
      expect(find.text(tr('settingsConflict')), findsOneWidget);
      expect(tester.widget<TextFormField>(key).controller!.text, isEmpty);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('provider editor excludes OAuth tokens', (tester) async {
    final node = NodeFixture('node', 'Node');
    final provider = {...node.provider, 'authentication': 'chat_gpt'};
    await pumpPage(
      tester,
      Scaffold(
        body: SingleChildScrollView(
          child: RecordForm(
            host: node.host,
            kind: 'providers',
            record: provider,
          ),
        ),
      ),
      session: AppSession.test(hosts: [node.host]),
    );
    expect(node.calls, isEmpty);
    expect(find.byKey(const ValueKey('settings-secret')), findsNothing);
  });

  testWidgets('memory edits preserve project ownership', (tester) async {
    final node = NodeFixture('node', 'Node');
    final entry = {
      'summary': {
        'id': 'memory',
        'project': 'project',
        'title': 'Conventions',
        'kind': 'project',
        'revision': 6,
        'updated_at_ms': 1234,
        'archived': false,
      },
      'body': 'Original\nUnicode 中文',
    };
    await pumpForm(
      tester,
      RecordForm(host: node.host, kind: 'memorySettings', record: entry),
      node,
    );
    await tester.enterText(
      find.byKey(const ValueKey('settings-content')),
      'Updated\nUnicode 中文',
    );
    await tap(tester, find.widgetWithText(FilledButton, tr('save')));
    final saved = node.calls.single.$2!;
    expect(saved['expected_revision'], 6);
    expect(saved['entry']['summary'], entry['summary']);
    expect(saved['entry']['body'], 'Updated\nUnicode 中文');
  });

  test('daily and weekly usage preserves cache and zero days', () {
    final days = [
      for (var i = 0; i < 9; i++)
        {
          'start_ms': DateTime.utc(2026, 9, 19 + i).millisecondsSinceEpoch,
          'metrics': {
            'responses': i == 0 ? 1 : 0,
            'tokens': i == 0
                ? {
                    'input': 100,
                    'cached_input': 40,
                    'output': 20,
                    'reasoning': 10,
                  }
                : null,
          },
        },
    ];
    final daily = usageBuckets(days);
    expect(daily.length, 9);
    expect(daily.first.tokens!.cache, 40);
    expect(daily.first.tokens!.input, 60);
    expect(daily.first.tokens!.output, 20);
    expect(daily.first.tokens!.total, 120);
    expect(daily.last.tokens!.total, 0);
    final weekly = usageBuckets(days, weekly: true);
    expect(weekly.length, 2);
    expect(weekly.first.end, DateTime.utc(2026, 9, 20));
    expect(weekly.last.start, DateTime.utc(2026, 9, 21));
    expect(weekly.first.tokens!.total, 120);
  });

  for (final brightness in [Brightness.light, Brightness.dark]) {
    testWidgets(
      'usage subscriptions, stacks, and filters in ${brightness.name}',
      (tester) async {
        final node = NodeFixture('node', 'Studio');
        final session = AppSession.test(hosts: [node.host]);
        final queries = <Map<String, dynamic>>[];
        final hosts = <String?>[];
        var closes = 0;
        Future<UsageWatch> watch(
          AppSession _,
          HostConnection? host,
          Map<String, dynamic> query,
        ) async {
          queries.add(query);
          hosts.add(host?.id);
          var first = true;
          final stopped = Completer<String>();
          final report = {
            'totals': {
              'responses': 1,
              'tokens': {'input': 100, 'cached_input': 40, 'output': 20},
              'cost': {'usd_micros': 5000, 'responses': 1},
            },
            'days': [
              for (var i = 0; i < 7; i++)
                {
                  'start_ms': (query['start_ms'] as int) + i * 86400000,
                  'metrics': {
                    'responses': i == 0 ? 1 : 0,
                    'tokens': i == 0
                        ? {'input': 100, 'cached_input': 40, 'output': 20}
                        : null,
                  },
                },
            ],
            'groups': [],
          };
          return UsageWatch(
            next: () async {
              if (first) {
                first = false;
                return jsonEncode(
                  host == null
                      ? {'summary': report, 'complete': true, 'error': null}
                      : {'report': report, 'error': null},
                );
              }
              return stopped.future;
            },
            close: () async {
              closes++;
              if (!stopped.isCompleted) {
                stopped.completeError(StateError('closed'));
              }
            },
          );
        }

        await pumpPage(
          tester,
          UsagePage(watch: watch),
          session: session,
          width: 320,
          brightness: brightness,
        );
        expect(
          tester.widget<Text>(find.byKey(const ValueKey('usage-total'))).data,
          '120',
        );
        final chart = tester.widget<BarChart>(find.byType(BarChart)).data;
        expect(
          chart.barGroups.first.barRods.first.rodStackItems.map(
            (s) => s.toY - s.fromY,
          ),
          [40, 60, 20],
        );
        await tap(tester, find.byKey(const ValueKey('usage-weekly')));
        expect(queries.length, 1);
        await tap(tester, find.text(tr('month')));
        expect(queries.length, 2);
        expect(
          queries.last['end_ms'] - queries.last['start_ms'],
          30 * 86400000,
        );
        await tap(
          tester,
          find.byTooltip('${tr('selectHost')}: ${tr('allHosts')}'),
        );
        await tap(tester, find.text('Studio'));
        expect(hosts.last, 'node');
        expect(closes, 2);
        await tap(tester, find.text(tr('allProjects')));
        await tap(tester, find.text('Studio project'));
        expect(queries.last['projects'], ['node-project']);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
        await tester.pump();
        expect(closes, 4);
      },
    );
  }
}
