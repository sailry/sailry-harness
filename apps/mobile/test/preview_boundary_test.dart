import 'dart:io';
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';

void main() {
  test('production sources never import preview fixtures', () {
    final directives = RegExp(r'''(?:import|export|part)\s+['"]([^'"]+)['"]''');
    final lib = Directory('lib').absolute;
    for (final file in lib.listSync(recursive: true).whereType<File>()) {
      if (!file.path.endsWith('.dart')) continue;
      for (final directive in directives.allMatches(file.readAsStringSync())) {
        final reference = directive.group(1)!;
        if (reference.startsWith('dart:')) continue;
        if (reference.startsWith('package:') &&
            !reference.startsWith('package:sailry_mobile/')) {
          continue;
        }
        final target = reference.startsWith('package:sailry_mobile/')
            ? File(
                '${lib.path}/${reference.substring('package:sailry_mobile/'.length)}',
              )
            : File.fromUri(file.uri.resolve(reference));
        expect(
          target.absolute.path.startsWith('${lib.path}/'),
          isTrue,
          reason: '${file.path} imports a non-production source: $reference',
        );
      }
    }
    expect(
      File('lib/features/resources/git_fixtures.dart').existsSync(),
      isFalse,
    );
    expect(File('preview/main.dart').existsSync(), isTrue);
    final catalog =
        jsonDecode(File('lib/l10n/app_en.arb').readAsStringSync())
            as Map<String, dynamic>;
    for (final key in [
      'approveContext',
      'chatHost',
      'workspaceSub',
      'diffSubtitle',
      'terminalSubtitle',
      'modelSource',
      'connectionOwner',
      'studio',
      'deleted',
      'reset',
      'footerNote',
      'assistantMessage',
      'queuedPreview',
      'followupTool',
      'toolsShort',
      'thoughtContent',
      'toolsComplete',
      'toolRead',
      'toolEdit',
      'attachmentNote',
      'sentPreview',
      'forked',
      'stopped',
      'featureBranch',
      'resourceSummary',
      'fileSample',
      'diffSummary',
      'diffFile',
      'diffPath',
      'terminalPlaceholder',
      'terminalPreview',
      'terminalOutput',
      'terminalControlNote',
      'costCoverage',
      'sourcesPartial',
      'aboutBody',
      'settingsSaved',
      'providerNote',
      'roleNote',
      'notificationsNote',
      'memoryNote',
      'reviewIntro',
      'reviewBoundaryText',
      'title',
      'answered',
      'archived',
      'server',
      'terminalFixture',
      'userMessage',
      'thoughtLive',
      'flowResult',
      'markdownExample',
    ]) {
      expect(catalog.containsKey(key), isFalse, reason: '$key is preview data');
    }
    expect(catalog['attachment'], 'Attachment');
    final fixtures =
        jsonDecode(File('preview/l10n/app_en.arb').readAsStringSync())
            as Map<String, dynamic>;
    expect(fixtures['attachment'], 'design-notes.md');
  });
}
