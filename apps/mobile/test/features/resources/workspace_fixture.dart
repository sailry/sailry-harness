import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

const scratchName = '00000000-0000-4000-8000-000000000001';
const scratch = {
  'id': 'scratch',
  'project': null,
  'path': '/fixture/workspaces/sessions/$scratchName',
};

HostConnection source({CommandHandler? command}) => HostConnection.test(
  id: 'host',
  label: 'Host',
  command: command,
  snapshot: {
    'projects': [
      {'id': 'project', 'name': 'User project', 'path': '/fixture/repository'},
    ],
    'worktrees': [
      scratch,
      {'id': 'orphan', 'project': 'removed', 'path': '/fixture/orphan'},
      {
        'id': 'linked',
        'project': 'project',
        'path': '/fixture/feature',
        'main': false,
      },
      {
        'id': 'main',
        'project': 'project',
        'path': '/fixture/repository',
        'main': true,
      },
    ],
  },
);

Widget screen(AppSession session, Widget child) => SessionScope(
  session: session,
  child: MaterialApp(
    theme: SailryTheme.of(Brightness.light),
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    home: Scaffold(body: child),
  ),
);

Future<void> release(WidgetTester tester, AppSession session) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await session.close();
  session.dispose();
}
